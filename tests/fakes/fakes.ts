/**
 * Fake external services for the parity suite (docs/native-rewrite-plan.md §5.4).
 *
 * One HTTPS server on 127.0.0.1 with a random port, certificate from tests/fakes/certs (the app
 * trusts that CA only in test mode). Each service lives under a path prefix; `hostMap()` tells the
 * app which real host goes to which prefix (APM_HOST_MAP). Services whose URL the user types
 * (IdP, Open WebUI) are addressed directly with the {FAKES} token in scenarios.
 *
 * Every request is recorded (method, path, selected headers, form/JSON body) so scenarios can
 * assert on what the app sent — and on what it must never send.
 */
import fs from 'node:fs';
import https from 'node:https';
import type { IncomingMessage, ServerResponse } from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const CERTS = path.join(path.dirname(fileURLToPath(import.meta.url)), 'certs');
export const TEST_CA_FILE = path.join(CERTS, 'ca.pem');

export interface FakesConfig {
  idp?: {
    /** username -> password */
    users?: Record<string, string>;
    /** "roleArn,principalArn" values for the SAML Role attribute. */
    roles?: string[];
    /** Account names shown on the AWS SAML sign-in page: accountId -> name. */
    accountNames?: Record<string, string>;
  };
  sts?: {
    /** Fail AssumeRoleWithSAML with this AWS error code (HTTP 400) or HTTP status. */
    errorCode?: string;
    errorStatus?: number;
    accessKeyId?: string;
    secretAccessKey?: string;
    sessionToken?: string;
    /** Fixed expiration (ISO); default now + DurationSeconds. */
    expiration?: string;
  };
  github?: {
    /** version (no "v") -> release body, or null for 404. */
    releases?: Record<string, string | null>;
  };
}

export interface RecordedRequest {
  method: string;
  path: string;
  service: string;
  headers: Record<string, string>;
  form?: Record<string, string>;
  json?: unknown;
}

export interface Fakes {
  baseUrl: string;
  requests: RecordedRequest[];
  hostMap(): Record<string, string>;
  close(): Promise<void>;
}

const RECORDED_HEADERS = ['user-agent', 'content-type', 'authorization', 'cookie', 'accept'];

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((resolve) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
  });
}

function cookies(req: IncomingMessage): Record<string, string> {
  return Object.fromEntries(
    (req.headers.cookie ?? '')
      .split(';')
      .map((c) => c.trim().split('='))
      .filter((p) => p[0])
      .map(([k, ...v]) => [k, v.join('=')])
  );
}

function send(res: ServerResponse, status: number, body: string, headers: Record<string, string | string[]> = {}): void {
  res.writeHead(status, { 'content-type': 'text/html; charset=utf-8', ...headers });
  res.end(body);
}

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');

// ---------------------------------------------------------------- SAML IdP (ADFS-like)

function loginForm(error?: string): string {
  return `<!doctype html><html><body>
<form method="post" id="loginForm" action="/idp/login">
${error ? `<div id="error">${esc(error)}</div>` : ''}
<input id="userNameInput" name="UserName" type="email" value="" />
<input id="passwordInput" name="Password" type="password" />
<input id="kmsiInput" name="Kmsi" type="checkbox" value="true" />
<input name="AuthMethod" type="hidden" value="FormsAuthentication" />
<input type="submit" value="Sign in" />
</form></body></html>`;
}

export function samlAssertionXml(roles: string[]): string {
  const values = roles.map((r) => `<saml:AttributeValue>${esc(r)}</saml:AttributeValue>`).join('');
  return `<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" ID="_fake" Version="2.0">
<saml:Assertion ID="_a" Version="2.0"><saml:AttributeStatement>
<saml:Attribute Name="https://aws.amazon.com/SAML/Attributes/RoleSessionName"><saml:AttributeValue>tester@example.test</saml:AttributeValue></saml:Attribute>
<saml:Attribute Name="https://aws.amazon.com/SAML/Attributes/Role">${values}</saml:Attribute>
</saml:AttributeStatement></saml:Assertion></samlp:Response>`;
}

async function idp(cfg: FakesConfig, req: IncomingMessage, res: ServerResponse, sub: string, rec: RecordedRequest): Promise<void> {
  const users = cfg.idp?.users ?? {};
  if (sub === '/sso' && req.method === 'GET') {
    // ADFS entry URL redirects to the form and sets a context cookie the POST must carry.
    return send(res, 302, '', { location: '/idp/login', 'set-cookie': 'MSISContext=ctx1; Path=/idp; Secure' });
  }
  if (sub === '/login' && req.method === 'GET') return send(res, 200, loginForm());
  if (sub === '/login' && req.method === 'POST') {
    const form = Object.fromEntries(new URLSearchParams(await readBody(req)));
    rec.form = { ...form, Password: form.Password ? '<redacted>' : '' };
    const ok = cookies(req).MSISContext === 'ctx1' && form.UserName in users && users[form.UserName] === form.Password;
    if (!ok) return send(res, 200, loginForm('Incorrect user ID or password.'));
    return send(res, 302, '', { location: '/idp/saml', 'set-cookie': 'MSISAuth=ok; Path=/idp; Secure' });
  }
  if (sub === '/saml' && req.method === 'GET') {
    if (cookies(req).MSISAuth !== 'ok') return send(res, 200, loginForm());
    const assertion = Buffer.from(samlAssertionXml(cfg.idp?.roles ?? [])).toString('base64');
    return send(
      res,
      200,
      `<!doctype html><html><body onload="document.forms[0].submit()">
<form method="POST" name="hiddenform" action="https://signin.aws.amazon.com:443/saml">
<input type="hidden" name="SAMLResponse" value="${assertion}" />
<noscript><input type="submit" value="Submit" /></noscript></form></body></html>`
    );
  }
  send(res, 404, 'not found');
}

// ---------------------------------------------------------------- AWS SAML sign-in page (account names)

async function samlSignin(cfg: FakesConfig, req: IncomingMessage, res: ServerResponse, rec: RecordedRequest): Promise<void> {
  const form = Object.fromEntries(new URLSearchParams(await readBody(req)));
  rec.form = { SAMLResponse: form.SAMLResponse ? '<assertion>' : '' };
  const rows = Object.entries(cfg.idp?.accountNames ?? {})
    .map(([id, name]) => `<div class="saml-account-name">Account: ${esc(name)} (${id})</div>`)
    .join('\n');
  send(res, 200, `<!doctype html><html><body><fieldset>${rows}</fieldset></body></html>`);
}

// ---------------------------------------------------------------- STS

async function sts(cfg: FakesConfig, req: IncomingMessage, res: ServerResponse, rec: RecordedRequest): Promise<void> {
  const form = Object.fromEntries(new URLSearchParams(await readBody(req)));
  rec.form = { ...form, SAMLAssertion: form.SAMLAssertion ? '<assertion>' : '' };
  const s = cfg.sts ?? {};
  const xml = (body: string, status = 200) => {
    res.writeHead(status, { 'content-type': 'text/xml', 'x-amzn-requestid': 'fake-request-id' });
    res.end(body);
  };
  if (s.errorStatus && !s.errorCode) return xml('<ErrorResponse/>', s.errorStatus);
  if (s.errorCode) {
    return xml(
      `<ErrorResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/"><Error><Type>Sender</Type><Code>${s.errorCode}</Code><Message>Fake ${s.errorCode}</Message></Error><RequestId>fake-request-id</RequestId></ErrorResponse>`,
      s.errorStatus ?? 400
    );
  }
  const duration = Number(form.DurationSeconds ?? 3600);
  const expiration = s.expiration ?? new Date(Date.now() + duration * 1000).toISOString().replace(/\.\d{3}Z$/, 'Z');
  const roleArn = form.RoleArn ?? 'arn:aws:iam::111122223333:role/Unknown';
  xml(`<AssumeRoleWithSAMLResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
<AssumeRoleWithSAMLResult>
<Credentials>
<AccessKeyId>${s.accessKeyId ?? 'ASIAFAKEACCESSKEY001'}</AccessKeyId>
<SecretAccessKey>${s.secretAccessKey ?? 'fakeSecretAccessKey/0123456789abcdefghijklmn'}</SecretAccessKey>
<SessionToken>${s.sessionToken ?? 'FakeSessionTokenForParitySuite'}</SessionToken>
<Expiration>${expiration}</Expiration>
</Credentials>
<AssumedRoleUser><AssumedRoleId>AROAFAKE:tester</AssumedRoleId><Arn>${esc(roleArn.replace(':role/', ':assumed-role/'))}/tester</Arn></AssumedRoleUser>
<Audience>https://signin.aws.amazon.com/saml</Audience>
<Issuer>https://idp.example.test</Issuer>
<Subject>tester</Subject><SubjectType>persistent</SubjectType><NameQualifier>fake</NameQualifier>
</AssumeRoleWithSAMLResult>
<ResponseMetadata><RequestId>fake-request-id</RequestId></ResponseMetadata>
</AssumeRoleWithSAMLResponse>`);
}

// ---------------------------------------------------------------- GitHub releases

function github(cfg: FakesConfig, res: ServerResponse, sub: string): void {
  const m = sub.match(/^\/repos\/[^/]+\/[^/]+\/releases\/tags\/v(.+)$/);
  const version = m ? decodeURIComponent(m[1]) : undefined;
  const body = version !== undefined ? cfg.github?.releases?.[version] : undefined;
  if (body === undefined || body === null) {
    res.writeHead(404, { 'content-type': 'application/json' });
    return void res.end(JSON.stringify({ message: 'Not Found' }));
  }
  res.writeHead(200, { 'content-type': 'application/json' });
  res.end(
    JSON.stringify({
      tag_name: `v${version}`,
      name: `v${version}`,
      body,
      html_url: `https://github.com/MichaelGermino/AWSProfileManager/releases/tag/v${version}`,
    })
  );
}

// ---------------------------------------------------------------- server

export async function startFakes(config: FakesConfig = {}): Promise<Fakes> {
  const requests: RecordedRequest[] = [];
  const server = https.createServer(
    { key: fs.readFileSync(path.join(CERTS, 'server.key')), cert: fs.readFileSync(path.join(CERTS, 'server.pem')) },
    async (req, res) => {
      const url = new URL(req.url ?? '/', 'https://127.0.0.1');
      const [, service = '', ...rest] = url.pathname.split('/');
      const sub = '/' + rest.join('/');
      const rec: RecordedRequest = {
        method: req.method ?? 'GET',
        path: url.pathname + url.search,
        service,
        headers: Object.fromEntries(RECORDED_HEADERS.filter((h) => req.headers[h]).map((h) => [h, String(req.headers[h])])),
      };
      requests.push(rec);
      try {
        switch (service) {
          case 'idp':
            return await idp(config, req, res, sub, rec);
          case 'saml-signin':
            return await samlSignin(config, req, res, rec);
          case 'sts':
            return await sts(config, req, res, rec);
          case 'github':
            return github(config, res, sub);
          default:
            return send(res, 404, 'unknown fake service');
        }
      } catch (err) {
        send(res, 500, String(err));
      }
    }
  );
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = (server.address() as { port: number }).port;
  const baseUrl = `https://127.0.0.1:${port}`;
  return {
    baseUrl,
    requests,
    hostMap: () => ({
      'sts.us-west-2.amazonaws.com': `${baseUrl}/sts`,
      'signin.aws.amazon.com': `${baseUrl}/saml-signin`,
      'api.github.com': `${baseUrl}/github`,
    }),
    close: () =>
      new Promise<void>((resolve) => {
        server.closeAllConnections();
        server.close(() => resolve());
      }),
  };
}
