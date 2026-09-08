import type { Profile } from '../../shared/types';
import { buildProfiles, type ImportableAccount, type ImportSource } from './importModel';

/**
 * Turn a picker selection into saved profiles, and fold the discovered account names into
 * settings.accountDisplayNames.
 *
 * Shared by both importers so name generation, section-name collision handling and display-name
 * seeding exist once. Returns the profiles created, for the wizard's summary step.
 */
export async function persistImport(input: {
  accounts: ImportableAccount[];
  selected: Set<string>;
  source: ImportSource;
  /** Remember the org/IdP so the profile form and a later import prefill. */
  rememberOrgDefaults?: boolean;
}): Promise<Profile[]> {
  const { accounts, selected, source, rememberOrgDefaults } = input;
  if (selected.size === 0) return [];

  const [settings, existingProfiles, defaultCreds] = await Promise.all([
    window.electron.getSettings(),
    window.electron.getProfiles(),
    window.electron.getDefaultCredentialsDisplay(),
  ]);

  const defaultHours = settings?.defaultSessionDurationHours ?? 1;

  const { profiles, displayNames, displayNamesChanged } = buildProfiles({
    accounts,
    selected,
    source,
    existingProfiles,
    existingDisplayNames: settings?.accountDisplayNames ?? {},
    // Matches the app's own floor; SSO credentials last an hour regardless, and the scheduler
    // refreshes on the expiry rule anyway.
    refreshIntervalMinutes: Math.max(60, Math.floor(defaultHours * 60)),
    hasStoredCredentials: !!defaultCreds?.hasPassword,
  });

  if (profiles.length === 0) return [];
  await window.electron.createProfiles(profiles);

  // Fill-if-empty throughout: importing a second org or IdP must not overwrite the first.
  const orgDefaults: Record<string, string> = {};
  if (rememberOrgDefaults) {
    if (source.kind === 'identityCenter') {
      if (!settings?.defaultSsoStartUrl?.trim()) orgDefaults.defaultSsoStartUrl = source.startUrl;
      if (!settings?.defaultSsoRegion?.trim()) orgDefaults.defaultSsoRegion = source.region;
    } else if (!settings?.defaultIdpEntryUrl?.trim()) {
      orgDefaults.defaultIdpEntryUrl = source.idpEntryUrl;
    }
  }

  if (settings && (displayNamesChanged || Object.keys(orgDefaults).length > 0)) {
    // Re-read rather than reusing the snapshot above: a step earlier in the wizard may have
    // written settings since, and this call replaces the whole object.
    const fresh = (await window.electron.getSettings()) ?? settings;
    await window.electron.saveSettings({
      ...fresh,
      accountDisplayNames: { ...fresh.accountDisplayNames, ...displayNames },
      ...orgDefaults,
    });
  }

  return profiles;
}

