// Host half of the answer window: keeps one small state file in sync with the
// agent's answer, its reasoning trace, and whether it is still working.
import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const stateDir = path.join(os.homedir(), '.dsh', 'activity');
const statePath = path.join(stateDir, 'state.json');
const viewerPath = path.join(here, 'viewer', 'ActivityWindow.exe');

const ANSWER_TAIL = 2200;
const THINK_TAIL = 500;

/** Keep the markdown intact; the window renders tables, code and lists itself. */
function tail(text, limit) {
  if (typeof text !== "string" || text.length === 0) return "";
  const cleaned = text.replace(/\r/g, "").replace(/[ \t]+$/gm, "").replace(/\n{3,}/g, "\n\n");
  if (cleaned.length <= limit) return cleaned;
  const cut = cleaned.slice(-limit);
  const nl = cut.indexOf("\n");
  return nl >= 0 && nl < 80 ? cut.slice(nl + 1) : cut;
}

export function apply(ctx, config = {}) {
  fs.mkdirSync(stateDir, { recursive: true });

  let answer = '';
  let thinking = '';
  let previous = '';
  let stepSince = Date.now();
  let streaming = false;
  let toolsRunning = 0;
  let lastEvent = Date.now();
  let viewer = null;
  let viewerStarted = false;

  const active = () => {
  // A missing tools/result (error, cancel) used to leave the counter stuck.
  if (toolsRunning > 0 && Date.now() - lastEvent > 20000) toolsRunning = 0;
  if (streaming && Date.now() - lastEvent > 120000) streaming = false;
  return streaming || toolsRunning > 0;
};

  const write = () => {
    const payload = JSON.stringify({
      ts: Date.now(),
      since: stepSince,
      active: active(),
      title: active() ? 'DeepSeek HARNESS · работаю' : 'DeepSeek HARNESS',
      answer: !active() ? '' : tail(answer, ANSWER_TAIL).length > 0
        ? tail(answer, ANSWER_TAIL)
        : (active()
            ? (thinking.trim().length > 0
                ? `Глубокое размышление… ${Math.max(1, Math.round((Date.now() - stepSince) / 1000))} с`
                : `Работаю… ${Math.max(1, Math.round((Date.now() - stepSince) / 1000))} с`)
            : ''),
      previous: '',
    });
    try {
      const temp = statePath + '.tmp';
      fs.writeFileSync(temp, payload, 'utf8');
      fs.renameSync(temp, statePath);
    } catch {
      // A missing state file only costs the window its update.
    }
  };

  let killed = false;
  const ensureViewer = () => {
    if (config.showViewer === false) return;
    if (viewer !== null && viewer.exitCode === null) return;
    if (!fs.existsSync(viewerPath)) return;
    const launch = () => {
      try {
        viewer = spawn(viewerPath, [statePath], { detached: true, stdio: "ignore" });
        viewer.on("exit", () => { viewer = null; });
        viewer.unref();
      } catch {
        // the state file still works without the window
      }
    };
    if (!killed) {
      // clear a stale copy left over from a previous plugin load
      killed = true;
      execFile("taskkill", ["/IM", "ActivityWindow.exe", "/F"], { windowsHide: true }, launch);
    } else {
      launch();
    }
  };

  write();
  ensureViewer();

  lastEvent = Date.now();
  ctx.on('agent/assistant-stream', (payload) => {
    try {
      const frame = payload?.frame;
      if (!frame) return;
      if (frame.type === 'start') {
        if (answer.trim().length > 0) previous = tail(answer, ANSWER_TAIL);
        answer = '';
        thinking = '';
        stepSince = Date.now();
        streaming = true;
        ensureViewer();
        write();
        return;
      }
      if (frame.type === 'chunk') {
        const chunk = frame.chunk;
        if (!chunk || typeof chunk.text !== 'string') return;
        if (chunk.type === 'reasoning-delta') {
          thinking += chunk.text;
        } else if (chunk.type === 'text-delta') {
          answer += chunk.text;
        } else {
          return;
        }
        streaming = true;
        write();
        return;
      }
      if (frame.type === 'end') {
        streaming = false;
        if (answer.trim().length > 0) previous = tail(answer, ANSWER_TAIL);
        write();
      }
    } catch {
      // never disturb the stream
    }
  });

  lastEvent = Date.now();
  ctx.on('tools/pre-execute', (exec, next) => {
    try {
      toolsRunning += 1;
      ensureViewer();
      write();
    } catch {
      // never disturb the tool pipeline
    }
    return next();
  });

  lastEvent = Date.now();
  ctx.on('tools/result', () => {
    try {
      toolsRunning = Math.max(0, toolsRunning - 1);
      write();
    } catch {
      // ignore
    }
  });

  lastEvent = Date.now();
  ctx.on('turn/end', () => {
    try {
      streaming = false;
      toolsRunning = 0;
      write();
    } catch {
      // ignore
    }
  });
}
