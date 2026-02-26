export interface Profile {
  id: string;
  name: string;
  idpEntryUrl: string;
  accountNumber?: string; // deprecated; use roleDisplayText / roleArn
  label: string;
  autoRefresh: boolean;
  /** Refresh interval in minutes (e.g. 60 = 1 hour). Used when autoRefresh is on. */
  refreshIntervalMinutes: number;
  useDefaultCredentials?: boolean;
  roleArn?: string;
  principalArn?: string;
  roleDisplayText?: string; // friendly label for selected role (e.g. "123456789012 / MyRole")
  expiration?: string; // ISO string
  credentialProfileName: string; // section in credentials file
  /** Icon identifier for profile list (e.g. 'user', 'briefcase'). Optional. */
  iconName?: string;
  /** Hex color for the profile icon (e.g. '#3b82f6'). Optional. */
  iconColor?: string;
}

export interface DashboardProfileSummary {
  id: string;
  name: string;
  accountNumber: string; // roleDisplayText or label for display
  label: string;
  status: 'active' | 'expired' | 'never';
  expiresAtPst?: string;
  timeRemainingSeconds?: number;
  iconName?: string;
  iconColor?: string;
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
  /** Open WebUI API base URL for Terminal AI Assistant (e.g. https://your-instance.com/api). No trailing slash. */
  openWebUiApiUrl?: string;
  /** Open WebUI API key for Terminal AI Assistant. Stored in settings; never sent to renderer. */
  openWebUiApiKey?: string;
  /** Open WebUI model name (e.g. Google Gemini 2.5 Flash-Lite). Optional; has default in AI service. */
  openWebUiModel?: string;
  /** Preferred embedded terminal shell: powershell or bash. Default powershell. */
  terminalShell?: 'powershell' | 'bash';
  /** Path to Bash executable for embedded terminal (e.g. Git Bash or WSL). Required when terminalShell is bash. */
  bashPath?: string;
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
