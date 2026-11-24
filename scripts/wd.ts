/**
 * Drive the running app over WebDriver, without the real mouse or focus.
 * Needs the app started with the dev-only feature:
 *   bun run tauri dev --features webdriver      # server on 127.0.0.1:4445
 *
 *   bun scripts/wd.ts OUT_DIR STEP...
 *     shot:NAME      screenshot of the webview (WebGL included) → OUT_DIR/NAME.png
 *     click:XPATH    click an element (a JS click; the cursor never moves)
 *     keys:TEXT      type into the focused element; a trailing \n is Enter
 *     wait:SECONDS   sleep
 *     js:SCRIPT      run JS in the page and print what it returns
 *
 *   bun scripts/wd.ts /tmp/shots \
 *     "click://button[@aria-label='Add .fe file']" \
 *     "click://div[@role='dialog']//button[normalize-space()='cube.fe']" wait:3 \
 *     "click://input[@aria-label='Surface Evolver command']" "keys:g 5\n" wait:2 shot:cube
 *
 * Type with keys:, not WebDriver's element send-keys: that sets the input's
 * value directly, React never sees a change, and Enter runs nothing.
 */
import { mkdirSync, writeFileSync } from "fs";

const BASE = `http://127.0.0.1:${process.env.TAURI_WEBDRIVER_PORT ?? 4445}`;
const ELEM = "element-6066-11e4-a52e-4f735466cecf";

async function call(method: string, path: string, body?: unknown): Promise<any> {
  const res = await fetch(BASE + path, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json: any = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`${method} ${path}: ${JSON.stringify(json.value ?? json)}`);
  return json.value;
}

// Elements can appear a moment after the step that causes them (a dialog, a load).
async function find(sid: string, xpath: string): Promise<string> {
  for (let i = 0; i < 50; i++) {
    try {
      return (await call("POST", `/session/${sid}/element`, { using: "xpath", value: xpath }))[ELEM];
    } catch {
      await Bun.sleep(200);
    }
  }
  throw new Error(`not found: ${xpath}`);
}

const [out, ...steps] = process.argv.slice(2);
if (!out) throw new Error("usage: bun scripts/wd.ts OUT_DIR STEP...  (see the header)");
mkdirSync(out, { recursive: true });

const sid: string = (await call("POST", "/session", { capabilities: {} })).sessionId;
try {
  for (const step of steps) {
    const i = step.indexOf(":");
    const [kind, arg] = [step.slice(0, i), step.slice(i + 1)];
    if (kind === "shot") {
      const png = Buffer.from(await call("GET", `/session/${sid}/screenshot`), "base64");
      writeFileSync(`${out}/${arg}.png`, png);
      console.log(`shot ${out}/${arg}.png`);
    } else if (kind === "click") {
      await call("POST", `/session/${sid}/element/${await find(sid, arg)}/click`, {});
    } else if (kind === "keys") {
      const text = arg.replace(/\\n$/, "");
      const actions = [...text].flatMap(ch => [{ type: "keyDown", value: ch }, { type: "keyUp", value: ch }]);
      await call("POST", `/session/${sid}/actions`, { actions: [{ type: "key", id: "kb", actions }] });
    } else if (kind === "wait") {
      await Bun.sleep(Number(arg) * 1000);
    } else if (kind === "js") {
      console.log(await call("POST", `/session/${sid}/execute/sync`, { script: arg, args: [] }));
    } else {
      throw new Error(`unknown step: ${step}`);
    }
  }
} finally {
  await call("DELETE", `/session/${sid}`);
}
