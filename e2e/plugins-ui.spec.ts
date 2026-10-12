import { expect, test, type Page } from "@playwright/test";
import { strToU8, zipSync } from "fflate";

// The plugin UI in the running app (lane 11): Install from .zip through the file picker,
// the permission dialog, a declarative panel with every field type, a plugin's prompt and
// confirm, the Plugins menu and a plugin shortcut, the developer console, Reload, Disable,
// installs that survive a reload, and Uninstall. The samples have their own spec
// (plugins-samples.spec.ts); the sandbox has plugins-sandbox.spec.ts.

const requireKernel = !!process.env.REQUIRE_KERNEL;
const ID = "org.example.everything";

const manifest = {
  id: ID,
  name: "Everything",
  version: "0.3.0",
  api: 1,
  description: "Every panel field, a prompt, a confirm, a command and a tool.",
  author: "Sawhorse",
  license: "MIT",
  permissions: ["ui", "storage"],
  contributes: {
    panels: [{ id: "main", title: "All fields" }],
    commands: [{ id: "ask", title: "Ask Me" }],
    tools: [{ id: "noop", title: "Noop", hint: "Does nothing.", vcb: "Anything" }],
    menus: [{ command: "ask" }],
    shortcuts: [{ command: "ask", key: "Alt+Shift+K" }],
  },
};

const PNG = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

const main = `// @ts-check
import { storage, ui } from "sawhorse";
const runs = ((await storage.app.get("runs")) ?? 0) + 1;
await storage.app.set("runs", runs);
console.log("started, run " + runs);
const panel = await ui.registerPanel({
  id: "main",
  body: [
    { type: "text", id: "name", label: "Name", value: "Shelf" },
    { type: "number", id: "len", label: "Length", unit: "length", value: 0.3048 },
    { type: "select", id: "wood", label: "Wood", value: "oak", options: [{ value: "oak", label: "Oak" }, { value: "ash", label: "Ash" }] },
    { type: "checkbox", id: "edge", label: "Edge band", value: false },
    { type: "table", id: "t", columns: ["Part", "Qty"], rows: [["Side", 2], ["Top", 1]], align: ["left", "right"] },
    { type: "note", id: "said", text: "Nothing yet." },
    { type: "image", id: "pic", src: ${JSON.stringify(PNG)}, alt: "A dot", width: 8 },
    { type: "button", id: "go", label: "Go", primary: true },
  ],
  onChange: (id, values) => panel.update({ fields: { said: { text: id + " is " + JSON.stringify(values[id]) } } }),
  onAction: (_id, values) => ui.notify("Go with " + values.name + ", " + values.len + " m, " + values.wood + ", " + values.edge),
});
await ui.registerCommand({ id: "ask", async run() {
  const name = await ui.prompt({ title: "Name it", label: "Name", value: "Box" });
  const ok = await ui.confirm({ title: "Sure?", text: "Use " + name + "?", ok: "Use it" });
  await ui.notify("Answers: " + name + " / " + ok);
} });
await ui.registerTool({ id: "noop", onValue: (text) => ui.notify("Typed " + text) });
`;

async function start(page: Page) {
  await page.goto("/?autosave=0&io=basic");
  const vp = page.locator("main.vp");
  await expect(vp).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  if (!requireKernel && (await vp.getAttribute("data-kernel")) === "missing") test.skip(true, "kernel WASM not built");
}

const row = (page: Page) => page.locator(`.plugin-row[data-plugin="${ID}"]`);
const hint = (page: Page) => page.locator("#hint");

test("install from .zip, enable, use every field, prompt and confirm, reload, disable, survive a reload, uninstall", async ({ page }) => {
  await start(page);
  await page.getByRole("tab", { name: "Plugins" }).click();
  // Install from .zip (zipped as a folder, the way people zip things).
  const zip = zipSync({ "everything/plugin.json": strToU8(JSON.stringify(manifest)), "everything/main.js": strToU8(main), "everything/README.md": strToU8("# Everything") });
  const [chooser] = await Promise.all([page.waitForEvent("filechooser"), page.locator('[data-do="install"]').click()]);
  await chooser.setFiles({ name: "everything.zip", mimeType: "application/zip", buffer: Buffer.from(zip) });
  await expect(hint(page)).toContainText("Everything 0.3.0 installed: enable it in the Plugins pane");
  await expect(row(page)).toHaveAttribute("data-status", "disabled");
  await expect(row(page)).toContainText("installed · Sawhorse · MIT");

  // The permission dialog, then running.
  await row(page).locator('[data-do="enable"]').click();
  const dialog = page.getByRole("dialog", { name: "Enable Everything" });
  await expect(dialog.locator("li[data-permission]")).toHaveText([/Add to the window\s*ui/, /Keep settings\s*storage/]);
  await dialog.getByRole("button", { name: "Enable" }).click();
  await expect(row(page)).toHaveAttribute("data-status", "running");

  // The panel: every field type, drawn by the app.
  const panel = page.locator(`[data-panel="${ID}/main"]`);
  await expect(panel.locator("summary")).toHaveText(/Everything\s*All fields/);
  await expect(panel.locator('[data-field="name"]')).toHaveValue("Shelf");
  await expect(panel.locator('[data-field="len"]')).toHaveValue("12");
  await expect(panel.locator('table[data-table="t"] tbody tr')).toHaveCount(2);
  await expect(panel.locator("img.pf-image")).toHaveAttribute("alt", "A dot");
  await panel.locator('[data-field="len"]').fill("18 1/2");
  await panel.locator('[data-field="len"]').press("Enter");
  await expect(panel.locator('[data-note="said"]')).toHaveText(`len is ${18.5 * 0.0254}`);
  // Enter that commits hands the keyboard back to the model: the next tool key picks a tool.
  const railTool = (id: string) => page.locator(`.rail button[data-tool="${id}"]`);
  await expect(panel.locator('[data-field="len"]')).not.toBeFocused();
  await page.keyboard.press("r");
  await expect(railTool("rect")).toHaveAttribute("aria-pressed", "true");
  await railTool("select").click();
  await panel.locator('[data-field="len"]').fill("nonsense");
  await panel.locator('[data-field="len"]').press("Enter");
  await expect(panel.locator(".pf-error")).toContainText("Can't read");
  await panel.locator('[data-field="len"]').press("Escape");
  await expect(panel.locator(".pf-error")).toHaveCount(0);
  await panel.locator('[data-field="wood"]').selectOption("ash");
  await expect(panel.locator('[data-note="said"]')).toHaveText('wood is "ash"');
  await panel.locator('[data-field="edge"]').click();
  await expect(panel.locator('[data-field="edge"]')).toBeChecked();
  await expect(panel.locator('[data-note="said"]')).toHaveText("edge is true");
  // A ticked box keeps the focus but not the letters: "p" picks Push/Pull.
  await expect(panel.locator('[data-field="edge"]')).toBeFocused();
  await page.keyboard.press("p");
  await expect(railTool("pushpull")).toHaveAttribute("aria-pressed", "true");
  await railTool("select").click();
  await panel.locator('[data-action="go"]').click();
  await expect(hint(page)).toContainText(`Everything: Go with Shelf, ${18.5 * 0.0254} m, ash, true`);

  // The Plugins menu: the command asks with the app's dialogs, titled with the plugin.
  await page.getByRole("button", { name: "Plugins", exact: true }).click();
  await page.getByRole("menuitem", { name: "Everything" }).click();
  await expect(page.getByRole("menuitem", { name: /Ask Me\s*Alt\+Shift\+K/ })).toBeVisible();
  await page.getByRole("menuitem", { name: /Ask Me/ }).click();
  const prompt = page.getByRole("dialog", { name: "Everything: Name it" });
  await expect(prompt).toContainText("from Everything");
  await prompt.getByRole("textbox").fill("Bench");
  await prompt.getByRole("textbox").press("Enter");
  const confirm = page.getByRole("dialog", { name: "Everything: Sure?" });
  await expect(confirm).toContainText("Use Bench?");
  await confirm.getByRole("button", { name: "Use it" }).click();
  await expect(hint(page)).toContainText("Everything: Answers: Bench / true");
  // The shortcut runs it too; Esc cancels the prompt.
  await page.locator("#cv").hover();
  await page.keyboard.press("Alt+Shift+K");
  await expect(page.getByRole("dialog", { name: "Everything: Name it" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("dialog", { name: "Everything: Sure?" }).getByRole("button", { name: "Cancel" }).click();
  await expect(hint(page)).toContainText("Everything: Answers: null / false");

  // Its tool sits in the rail's plugin group and takes the Value Control Box.
  const tool = page.locator(`.rail [data-group="plugins"] button[data-tool="plugin:${ID}/noop"]`);
  await tool.click();
  await expect(page.locator("#vcbName")).toHaveText("Anything");
  await page.locator("#vcb").fill("42");
  await page.locator("#vcb").press("Enter");
  await expect(hint(page)).toContainText("Everything: Typed 42");
  await page.keyboard.press("Space");

  // The developer console shows its log; Reload starts it again.
  await row(page).locator('[data-do="console"]').click();
  await expect(row(page).locator(".plugin-console")).toContainText("started, run 1");
  await row(page).locator('[data-do="reload"]').click();
  await expect(row(page)).toHaveAttribute("data-status", "running");
  await expect(row(page).locator(".plugin-console")).toContainText("started, run 2");

  // Installed and enabled plugins come back after a reload of the page.
  await page.reload();
  await expect(page.locator("main.vp")).not.toHaveAttribute("data-kernel", "loading", { timeout: 15_000 });
  await page.getByRole("tab", { name: "Plugins" }).click();
  await expect(row(page)).toHaveAttribute("data-status", "running", { timeout: 10_000 });
  await expect(page.locator(`[data-panel="${ID}/main"]`)).toBeVisible();

  // Disable: its panel, command and tool go.
  await row(page).locator('[data-do="disable"]').click();
  await expect(row(page)).toHaveAttribute("data-status", "disabled");
  await expect(page.locator(`[data-panel="${ID}/main"]`)).toHaveCount(0);
  await expect(page.locator(`.rail [data-group="plugins"]`)).toHaveCount(0);
  // Uninstall: gone, and still gone after a reload.
  await row(page).locator('[data-do="uninstall"]').click();
  await expect(row(page)).toHaveCount(0);
  await page.reload();
  await page.getByRole("tab", { name: "Plugins" }).click();
  await expect(page.locator(".plugin-row").first()).toBeVisible();
  await expect(row(page)).toHaveCount(0);
});

test("a broken zip is refused with its reasons, and a session load does not survive a reload", async ({ page }) => {
  await start(page);
  await page.getByRole("tab", { name: "Plugins" }).click();
  const bad = zipSync({ "plugin.json": strToU8(JSON.stringify({ ...manifest, id: "Not An Id", api: 2 })), "main.js": strToU8("") });
  let [chooser] = await Promise.all([page.waitForEvent("filechooser"), page.locator('[data-do="install"]').click()]);
  await chooser.setFiles({ name: "bad.zip", mimeType: "application/zip", buffer: Buffer.from(bad) });
  await expect(hint(page)).toContainText("Could not install the plugin");
  await expect(hint(page)).toContainText("reverse-DNS");
  const good = zipSync({ "plugin.json": strToU8(JSON.stringify(manifest)), "main.js": strToU8(main) });
  [chooser] = await Promise.all([page.waitForEvent("filechooser"), page.locator('[data-do="load-session"]').click()]);
  await chooser.setFiles({ name: "everything.zip", mimeType: "application/zip", buffer: Buffer.from(good) });
  await expect(row(page)).toContainText("this session");
  await page.reload();
  await page.getByRole("tab", { name: "Plugins" }).click();
  await expect(page.locator(".plugin-row").first()).toBeVisible();
  await expect(row(page)).toHaveCount(0);
});
