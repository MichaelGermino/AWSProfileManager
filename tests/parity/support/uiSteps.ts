/**
 * Electron implementation of the UI flow steps (native/tests/fixtures/flows/README.md).
 * Everything is located by ARIA role + accessible name, the same contract the GPUI runner meets
 * through Windows UI Automation, so one flow file drives both apps.
 */
import { expect, type Locator as PwLocator, type Page } from '@playwright/test';
import type { RunningApp } from './electronApp';
import type { Locator, Step, UiState } from './scenario';

export const UI_STEP_KEYS = ['click', 'hover', 'hold', 'fill', 'select', 'press', 'type', 'see'] as const;

export function isUiStep(step: Step): boolean {
  return UI_STEP_KEYS.some((k) => k in step);
}

function nameOption(name: string | undefined): string | RegExp | undefined {
  if (name === undefined) return undefined;
  return name.startsWith('$match:') ? new RegExp(name.slice('$match:'.length)) : name;
}

async function pageFor(running: RunningApp, l: Locator): Promise<Page> {
  if (!l.window || l.window === 'main') return running.page;
  const find = () => running.app.windows().find((w) => w.url().includes('auth-log-viewer'));
  for (let i = 0; i < 50 && !find(); i++) await new Promise((r) => setTimeout(r, 100));
  const p = find();
  if (!p) throw new Error(`window ${l.window} is not open`);
  return p;
}

async function resolve(running: RunningApp, l: Locator): Promise<PwLocator> {
  const page = await pageFor(running, l);
  const scope: Page | PwLocator = l.within ? await resolve(running, l.within) : page;
  const name = nameOption(l.name);
  let loc = scope.getByRole(l.role as Parameters<Page['getByRole']>[0], name === undefined ? {} : { name, exact: typeof name === 'string' });
  if (l.nth !== undefined) loc = loc.nth(l.nth);
  return loc;
}

const describe = (l: Locator) => `${l.role}${l.name !== undefined ? ` "${l.name}"` : ''}${l.nth !== undefined ? ` #${l.nth}` : ''}`;

async function expectState(loc: PwLocator, state: UiState): Promise<void> {
  switch (state) {
    case 'visible': return expect(loc).toBeVisible();
    case 'hidden': return expect(loc).toBeHidden();
    case 'enabled': return expect(loc).toBeEnabled();
    case 'disabled': return expect(loc).toBeDisabled();
    case 'checked': return expect(loc).toBeChecked();
    case 'unchecked': return expect(loc).not.toBeChecked();
    case 'selected': return expect(loc).toHaveAttribute('aria-selected', 'true');
    case 'expanded': return expect(loc).toHaveAttribute('aria-expanded', 'true');
    case 'collapsed': return expect(loc).toHaveAttribute('aria-expanded', 'false');
    case 'focused': return expect(loc).toBeFocused();
  }
}

export async function runUiStep(running: RunningApp, step: Step, tok: (v: unknown) => unknown): Promise<void> {
  const s = tok(step) as Record<string, unknown>;
  if ('click' in s) {
    const l = s.click as Locator;
    await (await resolve(running, l)).click({ button: (s.button as 'left' | 'right') ?? 'left' });
    return;
  }
  if ('hover' in s) return void (await (await resolve(running, s.hover as Locator)).hover());
  if ('hold' in s) {
    // Press-and-hold with the keyboard (Space), the way a keyboard or screen-reader user does it;
    // the GPUI runner does the same through UI Automation focus + key down/up.
    const loc = await resolve(running, s.hold as Locator);
    await loc.focus();
    await running.page.keyboard.down(' ');
    await running.page.waitForTimeout(Number(s.ms ?? 1200));
    await running.page.keyboard.up(' ');
    return;
  }
  if ('fill' in s) return void (await (await resolve(running, s.fill as Locator)).fill(String(s.value)));
  if ('select' in s) {
    const loc = await resolve(running, s.select as Locator);
    const want = String(s.option);
    if (!want.startsWith('$match:')) return void (await loc.selectOption({ label: want }));
    // Pick the first option whose label matches; wait for options to load (they often arrive async).
    const re = new RegExp(want.slice('$match:'.length));
    let label: string | undefined;
    await expect
      .poll(async () => {
        const labels = (await loc.locator('option').allTextContents()).map((t) => t.trim());
        label = labels.find((l) => re.test(l));
        return label !== undefined;
      }, { message: `option matching ${re} in ${describe(s.select as Locator)}` })
      .toBe(true);
    return void (await loc.selectOption({ label: label! }));
  }
  if ('press' in s) return void (await running.page.keyboard.press(String(s.press)));
  if ('type' in s) return void (await running.page.keyboard.type(String(s.type)));
  if ('see' in s) {
    const l = s.see as Locator;
    const loc = await resolve(running, l);
    const label = describe(l);
    if (s.count !== undefined) await expect(loc, label).toHaveCount(Number(s.count));
    if (s.state) await expectState(loc, s.state as UiState);
    else if (s.count === undefined) await expect(loc, label).toBeVisible();
    if (s.text !== undefined) {
      const t = String(s.text);
      await expect(loc, label).toContainText(t.startsWith('$match:') ? new RegExp(t.slice(7)) : t);
    }
    if (s.value !== undefined) await expect(loc, label).toHaveValue(String(s.value));
    return;
  }
  throw new Error(`unknown UI step ${JSON.stringify(step)}`);
}
