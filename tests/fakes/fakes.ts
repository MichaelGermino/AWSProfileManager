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
import crypto from 'node:crypto';
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
  sso?: {
    /** The region whose OIDC endpoint knows the start URL (detectRegion); default us-west-2. */
    region?: string;
    /** Identity returned in the id token (email claim). */
    identity?: string;
    accounts?: Array<{ accountId: string; accountName: string; emailAddress?: string; roles: string[] }>;
    /** Accounts per ListAccounts page (pagination); default all. */
    pageSize?: number;
    /** Access token lifetime in seconds; default 3600. Under 300 counts as already expired. */
    accessTokenTtl?: number;
    /** Refresh-token grant fails with invalid_grant. */
    refreshFails?: boolean;
    /** /authorize shows a page needing input instead of redirecting back (headless attempts time out). */
    authorizeNeedsInput?: boolean;
    /** /authorize redirects back with this OAuth error. */
    authorizeError?: string;
    /** Role credentials expiry as epoch ms; default now + 1h. */
    credentialsExpiration?: number;
  };
}

/** Regions the app may probe (detectRegion candidates plus common ones). */
export const SSO_REGIONS = ['us-east-1', 'us-west-2', 'us-east-2', 'us-west-1', 'eu-west-1'];

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

// ---------------------------------------------------------------- IAM Identity Center (OIDC + portal)

interface SsoState {
  clients: Map<string, { secret: string; redirectUris: string[] }>;
  codes: Map<string, { challenge: string; clientId: string; redirectUri: string }>;
  accessTokens: Set<string>;
  refreshTokens: Set<string>;
  seq: number;
}

const SECRET_KEYS = ['clientSecret', 'code', 'codeVerifier', 'refreshToken', 'accessToken'];
function redactJson(v: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, SECRET_KEYS.includes(k) && x ? '<redacted>' : x]));
}

function json(res: ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { 'content-type': 'application/json' });
  res.end(JSON.stringify(body));
}

function b64url(buf: Buffer): string {
  return buf.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

async function oidc(
  cfg: FakesConfig,
  st: SsoState,
  req: IncomingMessage,
  res: ServerResponse,
  region: string,
  sub: string,
  url: URL,
  rec: RecordedRequest
): Promise<void> {
  const sso = cfg.sso ?? {};
  const home = sso.region ?? 'us-west-2';
  if (req.method === 'POST') {
    const body = JSON.parse((await readBody(req)) || '{}') as Record<string, unknown>;
    rec.json = redactJson(body);
    if (sub === '/client/register') {
      const id = `client-${++st.seq}`;
      st.clients.set(id, { secret: `secret-${id}`, redirectUris: (body.redirectUris as string[]) ?? [] });
      return json(res, 200, {
        clientId: id,
        clientSecret: `secret-${id}`,
        clientIdIssuedAt: Math.floor(Date.now() / 1000),
        clientSecretExpiresAt: Math.floor(Date.now() / 1000) + 90 * 86400,
      });
    }
    if (sub === '/device_authorization') {
      if (region !== home) return json(res, 400, { error: 'invalid_request', error_description: 'Unknown start URL' });
      return json(res, 200, { deviceCode: 'dc', userCode: 'ABCD-EFGH', verificationUri: 'https://device.example.test', expiresIn: 600, interval: 1 });
    }
    if (sub === '/token') {
      const client = st.clients.get(String(body.clientId));
      if (!client || client.secret !== body.clientSecret) return json(res, 400, { error: 'invalid_client' });
      if (body.grantType === 'authorization_code') {
        const c = st.codes.get(String(body.code));
        const verifier = String(body.codeVerifier ?? '');
        const pkceOk = c && b64url(crypto.createHash('sha256').update(verifier).digest()) === c.challenge;
        if (!c || !pkceOk || c.clientId !== body.clientId || c.redirectUri !== body.redirectUri) {
          return json(res, 400, { error: 'invalid_grant', error_description: 'bad code or PKCE verifier' });
        }
        st.codes.delete(String(body.code));
      } else if (body.grantType === 'refresh_token') {
        if (sso.refreshFails || !st.refreshTokens.has(String(body.refreshToken))) {
          return json(res, 400, { error: 'invalid_grant', error_description: 'refresh token expired' });
        }
      } else {
        return json(res, 400, { error: 'unsupported_grant_type' });
      }
      const n = ++st.seq;
      const accessToken = `at-${n}`;
      const refreshToken = `rt-${n}`;
      st.accessTokens.add(accessToken);
      st.refreshTokens.add(refreshToken);
      const claims = b64url(Buffer.from(JSON.stringify({ email: sso.identity ?? 'tester@example.test' })));
      return json(res, 200, {
        accessToken,
        tokenType: 'Bearer',
        expiresIn: sso.accessTokenTtl ?? 3600,
        refreshToken,
        idToken: `${b64url(Buffer.from('{"alg":"none"}'))}.${claims}.`,
      });
    }
  }
  if (req.method === 'GET' && sub === '/authorize') {
    const q = Object.fromEntries(url.searchParams);
    rec.json = { ...q, code_challenge: q.code_challenge ? '<challenge>' : undefined, state: q.state ? '<state>' : undefined };
    const back = new URL(q.redirect_uri);
    if (sso.authorizeNeedsInput) return send(res, 200, '<!doctype html><html><body>Pick an account (fake)</body></html>');
    if (sso.authorizeError) {
      back.searchParams.set('error', sso.authorizeError);
      back.searchParams.set('state', q.state);
      return send(res, 302, '', { location: back.toString() });
    }
    const code = `code-${++st.seq}`;
    st.codes.set(code, { challenge: q.code_challenge, clientId: q.client_id, redirectUri: q.redirect_uri });
    back.searchParams.set('code', code);
    back.searchParams.set('state', q.state);
    return send(res, 302, '', { location: back.toString() });
  }
  send(res, 404, 'unknown oidc route');
}

function portal(cfg: FakesConfig, st: SsoState, req: IncomingMessage, res: ServerResponse, sub: string, url: URL): void {
  const token = String(req.headers['x-amz-sso_bearer_token'] ?? '');
  if (!st.accessTokens.has(token)) return json(res, 401, { message: 'Session token not found or invalid' });
  const sso = cfg.sso ?? {};
  const accounts = sso.accounts ?? [];
  const q = url.searchParams;
  if (sub === '/assignment/accounts') {
    const size = sso.pageSize ?? Math.max(accounts.length, 1);
    const start = Number(q.get('next_token') ?? 0);
    const page = accounts.slice(start, start + size).map(({ roles: _r, ...a }) => a);
    const next = start + size < accounts.length ? String(start + size) : undefined;
    return json(res, 200, { accountList: page, ...(next ? { nextToken: next } : {}) });
  }
  if (sub === '/assignment/roles') {
    const acct = accounts.find((a) => a.accountId === q.get('account_id'));
    return json(res, 200, { roleList: (acct?.roles ?? []).map((roleName) => ({ roleName, accountId: acct?.accountId })) });
  }
  if (sub === '/federation/credentials') {
    const acct = accounts.find((a) => a.accountId === q.get('account_id'));
    if (!acct || !acct.roles.includes(String(q.get('role_name')))) {
      return json(res, 403, { message: 'No access' });
    }
    return json(res, 200, {
      roleCredentials: {
        accessKeyId: 'ASIASSOFAKEKEY000001',
        secretAccessKey: 'ssoFakeSecretAccessKey/abcdefghijklmnop',
        sessionToken: 'SsoFakeSessionToken',
        expiration: sso.credentialsExpiration ?? Date.now() + 3600_000,
      },
    });
  }
  json(res, 404, { message: 'unknown portal route' });
}

// ---------------------------------------------------------------- server

export async function startFakes(config: FakesConfig = {}): Promise<Fakes> {
  const requests: RecordedRequest[] = [];
  const ssoState: SsoState = { clients: new Map(), codes: new Map(), accessTokens: new Set(), refreshTokens: new Set(), seq: 0 };
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
          case 'oidc': {
            // /oidc/<region>/<route>
            const [, region, ...r] = sub.split('/');
            rec.service = `oidc:${region}`;
            return await oidc(config, ssoState, req, res, region, '/' + r.join('/'), url, rec);
          }
          case 'portal': {
            const [, region, ...r] = sub.split('/');
            rec.service = `portal:${region}`;
            rec.headers['x-amz-sso_bearer_token'] = req.headers['x-amz-sso_bearer_token'] ? '<token>' : '';
            return portal(config, ssoState, req, res, '/' + r.join('/'), url);
          }
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
      ...Object.fromEntries(
        SSO_REGIONS.flatMap((r) => [
          [`oidc.${r}.amazonaws.com`, `${baseUrl}/oidc/${r}`],
          [`portal.sso.${r}.amazonaws.com`, `${baseUrl}/portal/${r}`],
        ])
      ),
    }),
    close: () =>
      new Promise<void>((resolve) => {
        server.closeAllConnections();
        server.close(() => resolve());
      }),
  };
}
