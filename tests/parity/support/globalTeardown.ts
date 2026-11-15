/**
 * Safety net after the whole suite: delete any keychain entry a crashed test left behind.
 * Only entries under the AWSProfileManager-test-<id> service are touched — never the real one.
 */
import { execFileSync } from 'node:child_process';

export default function globalTeardown(): void {
  if (process.platform !== 'win32') return; // macOS: per-test cleanup only (security CLI sweep TODO)
  const list = execFileSync('cmdkey', ['/list'], { encoding: 'utf8' });
  const targets = [...list.matchAll(/Target: (?:LegacyGeneric:target=)?(AWSProfileManager-test-[0-9a-f]{8}\/\S+)/g)].map((m) => m[1]);
  for (const t of targets) {
    try {
      execFileSync('cmdkey', [`/delete:${t}`], { stdio: 'ignore' });
    } catch {
      // already gone
    }
  }
  if (targets.length) console.log(`[teardown] removed ${targets.length} leftover test keychain entries`);
}
