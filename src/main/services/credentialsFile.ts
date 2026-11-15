import fs from 'fs';
import os from 'os';
import path from 'path';
import { getAwsCredentialsPath } from './appPaths';
import { parseCredentials, removeSection, renameSection, setSection } from './credentialsIni';

const CREDENTIALS_PATH = getAwsCredentialsPath();

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

function readText(): string {
  return fs.existsSync(CREDENTIALS_PATH) ? fs.readFileSync(CREDENTIALS_PATH, 'utf-8') : '';
}

export function readCredentialsFile(): Record<string, CredentialSection> {
  return parseCredentials(readText()) as Record<string, CredentialSection>;
}

/**
 * Writes one profile's section. Everything else in the file is left byte-for-byte as it was
 * (see credentialsIni.ts).
 */
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
  const next = setSection(
    readText(),
    profileName,
    {
      aws_access_key_id: credentials.aws_access_key_id,
      aws_secret_access_key: credentials.aws_secret_access_key,
      aws_session_token: credentials.aws_session_token,
      region: credentials.region || 'us-west-2',
      output: credentials.output || 'json',
    },
    os.EOL
  );
  fs.writeFileSync(CREDENTIALS_PATH, next, 'utf-8');
}

/**
 * Move a section to a new name, keeping its credentials. Used when a profile's
 * credentialProfileName changes: without this the live credentials stay stranded under the old
 * section and `--profile <new name>` fails until the user forces a refresh.
 *
 * Returns false when there was nothing to move.
 */
export function renameCredentialsSection(oldName: string, newName: string): boolean {
  const from = (oldName ?? '').trim();
  const to = (newName ?? '').trim();
  if (!from || !to || from === to) return false;
  if (!fs.existsSync(CREDENTIALS_PATH)) return false;
  const next = renameSection(readText(), from, to, os.EOL);
  if (next === null) return false;
  fs.writeFileSync(CREDENTIALS_PATH, next, 'utf-8');
  return true;
}

/** Remove a profile section from the credentials file (e.g. when a profile is deleted). */
export function removeCredentialsSection(profileName: string): void {
  if (!profileName?.trim()) return;
  if (!fs.existsSync(CREDENTIALS_PATH)) return;
  const next = removeSection(readText(), profileName, os.EOL);
  if (next !== null) fs.writeFileSync(CREDENTIALS_PATH, next, 'utf-8');
}
