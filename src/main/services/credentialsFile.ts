import fs from 'fs';
import path from 'path';
import ini from 'ini';

const CREDENTIALS_PATH =
  path.join(process.env.USERPROFILE || '', '.aws', 'credentials');

export interface CredentialSection {
  aws_access_key_id?: string;
  aws_secret_access_key?: string;
  aws_session_token?: string;
  region?: string;
  output?: string;
}

export function getCredentialsPath(): string {
  return CREDENTIALS_PATH;
}

export function readCredentialsFile(): Record<string, CredentialSection> {
  if (!fs.existsSync(CREDENTIALS_PATH)) {
    return {};
  }
  const raw = fs.readFileSync(CREDENTIALS_PATH, 'utf-8');
  const parsed = ini.parse(raw) as Record<string, CredentialSection>;
  return parsed;
}

export function writeCredentialsForProfile(
  profileName: string,
  credentials: {
    aws_access_key_id: string;
    aws_secret_access_key: string;
    aws_session_token: string;
    region?: string;
    output?: string;
  }
): void {
  const dir = path.dirname(CREDENTIALS_PATH);
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }

  const existing = readCredentialsFile();
  existing[profileName] = {
    aws_access_key_id: credentials.aws_access_key_id,
    aws_secret_access_key: credentials.aws_secret_access_key,
    aws_session_token: credentials.aws_session_token,
    region: credentials.region || 'us-west-2',
    output: credentials.output || 'json',
  };

  const content = ini.stringify(existing);
  fs.writeFileSync(CREDENTIALS_PATH, content, 'utf-8');
}

/** Remove a profile section from the credentials file (e.g. when a profile is deleted). */
export function removeCredentialsSection(profileName: string): void {
  if (!profileName?.trim()) return;
  if (!fs.existsSync(CREDENTIALS_PATH)) return;
  const existing = readCredentialsFile();
  if (!(profileName in existing)) return;
  delete existing[profileName];
  const content = ini.stringify(existing);
  fs.writeFileSync(CREDENTIALS_PATH, content, 'utf-8');
}
