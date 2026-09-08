/** How a profile obtains credentials. Absent means 'saml' — existing profiles predate this field. */
export type ProfileAuthType = 'saml' | 'identityCenter';

export interface Profile {
  id: string;
  name: string;
  idpEntryUrl: string;
  accountNumber?: string; // deprecated; use roleDisplayText / roleArn
  label: string;
  autoRefresh: boolean;
  /** undefined is treated as 'saml' everywhere; resolve with resolveAuthType(). */
  authType?: ProfileAuthType;
  /** Identity Center: access portal start URL, no '#/' fragment (e.g. https://d-xxxx.awsapps.com/start). */
  ssoStartUrl?: string;
  /** Identity Center: region of the Identity Center instance (e.g. 'us-west-2'). */
  ssoRegion?: string;
  /** Identity Center: 12-digit AWS account id. */
  ssoAccountId?: string;
  /** Identity Center: permission set name (what ListAccountRoles calls roleName). */
  ssoRoleName?: string;
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
  /** Open WebUI model id, exactly as the instance reports it (e.g. "Google Gemini 3.5 Flash").
   *  Required once the URL and key are set — model ids are instance-specific, so there is
   *  deliberately no fallback default in the AI service. */
  openWebUiModel?: string;
  /** Preferred embedded terminal shell: powershell or bash. Default powershell. */
  terminalShell?: 'powershell' | 'bash';
  /** Profile pre-selected in the Terminal screen's profile dropdown.
   *  Three distinct states, so "never chosen" stays different from "chose none":
   *    undefined -> not chosen yet; the first available profile is used
   *    ''        -> explicitly "No profile"
   *    <id>      -> that profile (falls back to the first if the id no longer exists)
   *  Resolve with resolveTerminalProfileId() rather than reading this directly. */
  defaultTerminalProfileId?: string;
  /** Path to Bash executable for embedded terminal (e.g. Git Bash or WSL). Required when terminalShell is bash. */
  bashPath?: string;
  /** True when user has set a master password; credentials are stored encrypted. Not the password itself. */
  masterPasswordEnabled?: boolean;
  /** When not false, IdP auth attempts and failures are written to auth-audit-log.json (default: on). */
  authLoggingEnabled?: boolean;
  /** Where Identity Center sign-in happens. Default 'embedded': an in-app window on a per-org
   *  persistent partition, isolated from the default browser's session (which in orgs that issue
   *  separate privileged accounts is signed in as the wrong identity). Switch to 'external' if
   *  Conditional Access rejects embedded webviews. */
  ssoBrowserMode?: 'embedded' | 'external';
  /** Where the one-click AWS console sign-in opens. Default 'embedded': an in-app window on a
   *  per-profile partition, so each account keeps its own console session (the AWS console allows
   *  only one session per browser). 'external' uses the default browser, which fails with
   *  "You must first log out" unless AWS multi-session is enabled in that browser. */
  consoleBrowserMode?: 'embedded' | 'external';
  /** Which installed browser 'external' mode uses. 'default' (or unset) uses the OS default. */
  consoleBrowser?: string;
  /** Set once the AWS multi-session opt-in has been run from this app. Opt-in is browser-scoped
   *  and returns HTTP 400 if repeated while a session is live, so it must not run every time. */
  consoleMultiSessionOptInDone?: boolean;
  /** Default SSO region offered when adding an Identity Center profile. */
  defaultSsoRegion?: string;
  /** Default SSO start URL offered when adding an Identity Center profile. */
  defaultSsoStartUrl?: string;
  /** Developer-only: when true, the in-app updater will consider GitHub pre-releases (semver tags with a hyphen, e.g. 1.2.4-rc.1).
   *  Default: false. Toggle from Settings → Debug → Developer options after the 7-click unlock. */
  allowPrerelease?: boolean;
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

/** An Identity Center profile whose SSO session cannot be renewed silently. Not a failure:
 *  it means a human must complete a browser sign-in. Callers must not count it as an error. */
export interface SsoLoginRequiredResult {
  ssoLoginRequired: true;
  profileId: string;
  startUrl: string;
  region: string;
}

export type RefreshResult =
  | { success: true }
  | { success: false; error: string }
  | CredentialsRequiredResult
  | AuthRolesResult
  | SsoLoginRequiredResult;

/** One account from the Identity Center portal, with the permission sets assigned to the user. */
export interface SsoAccount {
  accountId: string;
  accountName: string;
  emailAddress?: string;
  roles: string[];
}

export interface SsoSessionStatus {
  signedIn: boolean;
  /** ISO string; when the current access token expires (renewed silently well before this). */
  expiresAt?: string;
  /** Whatever the token tells us about who signed in, for catching wrong-account sign-ins. */
  identity?: string;
}

/** Progress of an interactive SSO sign-in, pushed to the renderer while a window is open. */
export type SsoLoginProgress =
  | { phase: 'opening'; startUrl: string }
  | { phase: 'waiting'; startUrl: string }
  | { phase: 'deviceCode'; startUrl: string; userCode: string; verificationUri: string }
  | { phase: 'done'; startUrl: string; identity?: string }
  | { phase: 'failed'; startUrl: string; error: string };
