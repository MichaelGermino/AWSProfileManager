/**
 * Runs the fake services as a child process for the Rust scenario runner
 * (native/crates/apm-core/tests/scenarios.rs). Node 24 runs this file directly (type stripping).
 *
 *   node tests/fakes/serve.ts '<FakesConfig JSON>'
 *
 * Prints one JSON line: { baseUrl, hostMap, caFile, control }. `control` is a plain-HTTP endpoint
 * on 127.0.0.1: GET /requests returns what the fakes recorded, POST /reset clears it. The process
 * exits when its stdin closes, so a crashed test never leaves it behind.
 */
import http from 'node:http';
import { startFakes, TEST_CA_FILE, type FakesConfig } from './fakes.ts';

const config = JSON.parse(process.argv[2] ?? '{}') as FakesConfig;
const fakes = await startFakes(config);

const control = http.createServer((req, res) => {
  if (req.method === 'GET' && req.url === '/requests') {
    res.writeHead(200, { 'content-type': 'application/json' });
    return void res.end(JSON.stringify(fakes.requests));
  }
  if (req.method === 'POST' && req.url === '/reset') {
    fakes.requests.length = 0;
    res.writeHead(204);
    return void res.end();
  }
  res.writeHead(404);
  res.end();
});
await new Promise<void>((resolve) => control.listen(0, '127.0.0.1', resolve));
const port = (control.address() as { port: number }).port;

process.stdout.write(
  JSON.stringify({ baseUrl: fakes.baseUrl, hostMap: fakes.hostMap(), caFile: TEST_CA_FILE, control: `http://127.0.0.1:${port}` }) + '\n'
);

process.stdin.resume();
process.stdin.on('end', async () => {
  control.close();
  await fakes.close();
  process.exit(0);
});
