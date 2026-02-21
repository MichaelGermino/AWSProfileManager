import { shell } from 'electron';
import path from 'path';
import { getProfiles } from './profileStorage';

const SERVICE_NAME = 'AWSProfileManager';
export const DEFAULT_CREDENTIALS_ID = '__default__';

function getKeytar(): typeof import('keytar') | null {
  try {
    return require('keytar');
  } catch {
    return null;
  }
}

export async function getStoredCredentials(profileId: string): Promise<{ username: string; password: string } | null> {
  const keytar = getKeytar();
  if (!keytar) return null;
  const password = await keytar.getPassword(SERVICE_NAME, profileId);
  const username = await keytar.getPassword(SERVICE_NAME, `${profileId}_username`);
  if (profileId === DEFAULT_CREDENTIALS_ID) {
    if (username === null && password === null) return null;
    return { username: username || '', password: password || '' };
  }
  if (!password) return null;
  return {
    username: username || '',
    password,
  };
}

/** Returns only the default username (for display in Settings). Never returns password. */
export async function getDefaultCredentialsDisplay(): Promise<{ username: string; hasPassword: boolean } | null> {
  const keytar = getKeytar();
  if (!keytar) return null;
  const username = await keytar.getPassword(SERVICE_NAME, `${DEFAULT_CREDENTIALS_ID}_username`);
  const hasPassword = !!(await keytar.getPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID));
  if (username === null && !hasPassword) return null;
  return { username: username || '', hasPassword };
}

/** Save default credentials. Password can be empty (username-only). Leave password empty to keep existing. */
export async function setDefaultCredentials(username: string, password: string | null): Promise<void> {
  const keytar = getKeytar();
  if (!keytar) return;
  await keytar.setPassword(SERVICE_NAME, `${DEFAULT_CREDENTIALS_ID}_username`, username);
  if (password !== null) await keytar.setPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID, password);
}

export async function forgetDefaultCredentials(): Promise<void> {
  await deleteStoredCredentials(DEFAULT_CREDENTIALS_ID);
}

export async function setStoredCredentials(
  profileId: string,
  username: string,
  password: string
): Promise<void> {
  const keytar = getKeytar();
  if (!keytar) return;
  await keytar.setPassword(SERVICE_NAME, `${profileId}_username`, username);
  await keytar.setPassword(SERVICE_NAME, profileId, password);
}

export async function deleteStoredCredentials(profileId: string): Promise<void> {
  const keytar = getKeytar();
  if (!keytar) return;
  await keytar.deletePassword(SERVICE_NAME, profileId);
  await keytar.deletePassword(SERVICE_NAME, `${profileId}_username`);
}

export async function getCredentialsStatus(): Promise<{ profileId: string; hasCredentials: boolean }[]> {
  const profiles = getProfiles();
  const keytar = getKeytar();
  const result: { profileId: string; hasCredentials: boolean }[] = [];
  for (const p of profiles) {
    let hasCredentials = false;
    if (keytar) {
      const pw = await keytar.getPassword(SERVICE_NAME, p.id);
      hasCredentials = !!pw;
    }
    result.push({ profileId: p.id, hasCredentials });
  }
  return result;
}

export async function forgetCredentials(profileId: string): Promise<void> {
  await deleteStoredCredentials(profileId);
}

export async function openCredentialsFile(): Promise<void> {
  const credentialsPath = path.join(process.env.USERPROFILE || '', '.aws', 'credentials');
  await shell.openPath(credentialsPath);
}
