import { dialog } from 'electron';
import fs from 'fs';
import type { BrowserWindow, OpenDialogOptions } from 'electron';
import {
  ORG_CONFIG_KIND,
  ORG_CONFIG_VERSION,
  parseOrgConfig,
  type OrgConfig,
} from '../../shared/orgConfig';
import { getSettings } from './settingsService';

/**
 * Export/import of the shareable organization configuration.
 *
 * Distinct from configBackup: that captures one user's whole state including their profiles, this
 * captures only the org-wide settings a colleague would otherwise be told by hand. It is built
 * field by field from settings rather than by spreading them, so a secret can never be included by
 * accident when new settings are added later — notably openWebUiApiKey, which sits right next to
 * the URL that IS exported.
 */

export type ExportOrgConfigResult =
  | { canceled: true }
  | { success: true; path: string }
  | { success: false; error: string };

export type ImportOrgConfigResult =
  | { canceled: true }
  | { success: true; config: OrgConfig }
  | { success: false; error: string };

export function buildOrgConfig(organizationName?: string): OrgConfig {
  const s = getSettings();
  return {
    kind: ORG_CONFIG_KIND,
    version: ORG_CONFIG_VERSION,
    ...(organizationName?.trim() ? { organizationName: organizationName.trim() } : {}),
    ...(s.defaultIdpEntryUrl?.trim() ? { idpEntryUrl: s.defaultIdpEntryUrl.trim() } : {}),
    ...(s.defaultSsoStartUrl?.trim() ? { ssoStartUrl: s.defaultSsoStartUrl.trim() } : {}),
    ...(s.defaultSsoRegion?.trim() ? { ssoRegion: s.defaultSsoRegion.trim() } : {}),
    ...(s.openWebUiApiUrl?.trim() ? { openWebUiApiUrl: s.openWebUiApiUrl.trim() } : {}),
    ...(s.openWebUiModel?.trim() ? { openWebUiModel: s.openWebUiModel.trim() } : {}),
    ...(Object.keys(s.accountDisplayNames ?? {}).length
      ? { accountDisplayNames: { ...s.accountDisplayNames } }
      : {}),
  };
}

export async function exportOrgConfig(
  mainWindow: BrowserWindow | null,
  organizationName?: string
): Promise<ExportOrgConfigResult> {
  const opts = {
    title: 'Export organization configuration',
    defaultPath: 'aws-profile-manager-org-config.json',
    filters: [{ name: 'JSON', extensions: ['json'] }],
  };
  const result = mainWindow
    ? await dialog.showSaveDialog(mainWindow, opts)
    : await dialog.showSaveDialog(opts);
  if (result.canceled || !result.filePath) return { canceled: true };

  try {
    fs.writeFileSync(result.filePath, JSON.stringify(buildOrgConfig(organizationName), null, 2), 'utf-8');
    return { success: true, path: result.filePath };
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : String(err) };
  }
}

export async function importOrgConfig(mainWindow: BrowserWindow | null): Promise<ImportOrgConfigResult> {
  const openOpts: OpenDialogOptions = {
    title: 'Open organization configuration',
    filters: [{ name: 'JSON', extensions: ['json'] }],
    properties: ['openFile'],
  };
  const result = mainWindow
    ? await dialog.showOpenDialog(mainWindow, openOpts)
    : await dialog.showOpenDialog(openOpts);
  if (result.canceled || !result.filePaths?.length) return { canceled: true };

  try {
    const parsed = parseOrgConfig(JSON.parse(fs.readFileSync(result.filePaths[0], 'utf-8')));
    if (!parsed) {
      return {
        success: false,
        error: 'That file is not an organization configuration exported from this app.',
      };
    }
    return { success: true, config: parsed };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { success: false, error: `Could not read the file: ${message}` };
  }
}
