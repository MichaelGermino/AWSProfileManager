export interface Profile {
  id: string;
  name: string;
  idpEntryUrl: string;
  accountNumber?: string; // deprecated; use roleDisplayText / roleArn
  label: string;
  autoRefresh: boolean;
  refreshIntervalHours: number;
  useDefaultCredentials?: boolean;
  roleArn?: string;
  principalArn?: string;
  roleDisplayText?: string; // friendly label for selected role (e.g. "123456789012 / MyRole")
  expiration?: string; // ISO string
  credentialProfileName: string; // section in credentials file
}

export interface DashboardProfileSummary {
  id: string;
  name: string;
  accountNumber: string; // roleDisplayText or label for display
  label: string;
  status: 'active' | 'expired' | 'never';
  expiresAtPst?: string;
  timeRemainingSeconds?: number;
}

export interface Settings {
  defaultSessionDurationHours: number;
  defaultIdpEntryUrl: string;
  launchAtStartup: boolean;
  startMinimizedToTray: boolean;
  /** Map AWS account ID to friendly display name. Used in role dropdown. */
  accountDisplayNames?: Record<string, string>;
  /** Default template for account display names; stored in settings.json under accountDisplayNamesDefault. "Restore defaults" copies this to accountDisplayNames. */
  accountDisplayNamesDefault?: Record<string, string>;
}

export interface AwsRole {
  roleArn: string;
  principalArn: string;
  displayText: string;
  /** Friendly account name from IdP (e.g. from element with class saml-account-name on the page). */
  accountName?: string;
}

export interface CredentialsRequiredResult {
  required: true;
  profileId: string;
}

export interface AuthRolesResult {
  roles: AwsRole[];
  profileId: string;
}

export type RefreshResult =
  | { success: true }
  | { success: false; error: string }
  | CredentialsRequiredResult;
