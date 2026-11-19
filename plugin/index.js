// Host half of the answer window: keeps one small state file in sync with the
// agent's answer, its reasoning trace, and whether a turn is still running.
import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const stateDir = path.join(os.homedir(), '.dsh', 'activity');
const statePath = path.join(stateDir, 'state.json');
const viewerPath = path.join(here, 'viewer', 'ActivityWindow.exe');

const ANSWER_LIMIT = 2200;
const THINK_LIMIT = 400;
const TOOL_STALE_MS = 20000;
const STREAM_STALE_MS = 120000;

/** Keep the markdown intact; the window renders it. */
function clip(text, limit) {
  if (typeof text !== 'string' || text.length === 0) return '';
  const clean = text.replace(/\r/g, '').replace(/[ \t]+$/gm, '').replace(/\n{3,}/g, '\n\n');
  if (clean.length <= limit) return clean;
  const tail = clean.slice(-limit);
  const nl = tail.indexOf('\n');
  return nl >= 0 && nl < 80 ? tail.slice(nl + 1) : tail;
}

export function apply(ctx, config = {}) {
  fs.mkdirSync(stateDir, { recursive: true });

  let answer = '';
  let thinking = '';
  let stepSince = Date.now();
  let streaming = false;
  let toolsRunning = 0;
  let inTurn = false;
  let lastEvent = Date.now();
  let viewer = null;
  let killedOnce = false;

  const working = () => {
    // A tool that never reports back must not pin the panel forever.
    if (toolsRunning > 0 && Date.now() - lastEvent > TOOL_STALE_MS) toolsRunning = 0;
    if (streaming && Date.now() - lastEvent > STREAM_STALE_MS) streaming = false;
    return inTurn || streaming || toolsRunning > 0;
  };

  const write = () => {
    const active = working();
    const seconds = Math.max(1, Math.round((Date.now() - stepSince) / 1000));
    const prose = clip(answer, ANSWER_LIMIT);
    const thoughts = clip(thinking, THINK_LIMIT);

    let body = '';
    if (prose.length > 0) {
      body = prose;
    } else if (active) {
      body = thoughts.length > 0
        ? `Глубокое размышление… ${seconds} с`
        : `Работаю… ${seconds} с`;
    }

    const payload = JSON.stringify({
      ts: Date.now(),
      since: stepSince,
      active,
      title: 'DeepSeek HARNESS',
      answer: body,
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

  const ensureViewer = () => {
    if (config.showViewer === false) return;
    if (viewer !== null && viewer.exitCode === null) return;
    if (!fs.existsSync(viewerPath)) return;

    const launch = () => {
      try {
        viewer = spawn(viewerPath, [statePath], { detached: true, stdio: 'ignore' });
        viewer.on('exit', () => { viewer = null; });
        viewer.unref();
      } catch {
        // The state file still works without the window.
      }
    };

    if (!killedOnce) {
      killedOnce = true;
      execFile('taskkill', ['/IM', 'ActivityWindow.exe', '/F'], { windowsHide: true }, launch);
    } else {
      launch();
    }
  };

  write();
  ensureViewer();

  ctx.on('agent/assistant-stream', (payload) => {
    try {
      const frame = payload?.frame;
      if (!frame) return;
      lastEvent = Date.now();

      if (frame.type === 'start') {
        answer = '';
        thinking = '';
        stepSince = Date.now();
        streaming = true;
        inTurn = true;
        ensureViewer();
        write();
        return;
      }
      if (frame.type === 'chunk') {
        const chunk = frame.chunk;
        if (!chunk || typeof chunk.text !== 'string') return;
        if (chunk.type === 'text-delta') {
          answer += chunk.text;
        } else if (chunk.type === 'reasoning-delta') {
          thinking += chunk.text;
        } else {
          return;
        }
        streaming = true;
        inTurn = true;
        write();
        return;
      }
      if (frame.type === 'end') {
        streaming = false;
        write();
      }
    } catch {
      // never disturb the stream
    }
  });

  ctx.on('tools/pre-execute', (exec, next) => {
    try {
      lastEvent = Date.now();
      toolsRunning += 1;
      inTurn = true;
      ensureViewer();
      write();
    } catch {
      // never disturb the tool pipeline
    }
    return next();
  });

  ctx.on('tools/result', () => {
    try {
      lastEvent = Date.now();
      toolsRunning = Math.max(0, toolsRunning - 1);
      write();
    } catch {
      // ignore
    }
  });

  ctx.on('turn/end', () => {
    try {
      streaming = false;
      toolsRunning = 0;
      inTurn = false;
      write();
    } catch {
      // ignore
    }
  });
}
