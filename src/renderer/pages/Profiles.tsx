import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import type {
  Profile,
  ProfileFolder,
  DashboardProfileSummary,
  AwsRole,
  SsoAccount,
  ProfileAuthType,
  Settings,
} from '../../shared/types';
import { normalizeStartUrl, resolveAuthType } from '../../shared/ssoOrg';
import { groupProfilesByFolder, flattenGrouping } from '../../shared/profileGrouping';
import { v4 as uuidv4 } from 'uuid';
import { Tooltip } from '../components/Tooltip';
import { ProfileIconPicker } from '../components/ProfileIconPicker';
import { ProfileAvatar } from '../components/ProfileAvatar';
import {
  ProfileFolderSection,
  FOLDER_SORTABLE_PREFIX,
  FOLDER_CONTAINER_PREFIX,
  UNGROUPED_CONTAINER_ID,
} from '../components/ProfileFolderSection';
import { FloatingMenu, menuAnchorFor } from '../components/FloatingMenu';

declare global {
  interface Window {
    electron: {
      getProfiles: () => Promise<Profile[]>;
      getProfileById: (id: string) => Promise<Profile | null>;
      getDashboardState: () => Promise<DashboardProfileSummary[]>;
      applyProfileLayout: (
        orderedIds: string[],
        folderByProfileId: Record<string, string | null>
      ) => Promise<void>;
      getFolders: () => Promise<ProfileFolder[]>;
      saveFolder: (folder: Partial<ProfileFolder>) => Promise<ProfileFolder>;
      deleteFolder: (id: string) => Promise<void>;
      reorderFolders: (orderedIds: string[]) => Promise<void>;
      getCollapsedFolders: () => Promise<string[]>;
      setCollapsedFolders: (ids: string[]) => Promise<void>;
      getSettings: () => Promise<Settings>;
      saveSettings: (settings: Settings) => Promise<void>;
      saveProfile: (profile: Profile) => Promise<void>;
      deleteProfile: (id: string) => Promise<void>;
      refreshProfile: (profileId: string) => Promise<unknown>;
      submitCredentials: (profileId: string, username: string, password: string) => Promise<unknown>;
      selectRole: (profileId: string, roleIndex: number) => Promise<unknown>;
      onCredentialsRequired: (cb: (profileId: string, prefillUsername?: string) => void) => void;
      onCredentialsRefreshed: (cb: (profileId: string) => void) => void;
      onRefreshStarted: (cb: (profileId: string) => void) => void;
      onCredentialsExpired: (cb: (profileId: string, message: string) => void) => void;
      onNetworkUnavailable: (cb: (profileId: string) => void) => void;
      getCachedRoles: (idpEntryUrl: string) => Promise<AwsRole[] | null>;
      fetchRoles: (idpEntryUrl: string, useDefaultCredentials: boolean, profileId?: string) => Promise<unknown>;
      fetchRolesWithCredentials: (idpEntryUrl: string, username: string, password: string) => Promise<unknown>;
      getDefaultCredentialsDisplay: () => Promise<{ username: string; hasPassword: boolean; locked?: boolean } | null>;
      getSidebarCollapsed: () => Promise<boolean>;
      setSidebarCollapsed: (collapsed: boolean) => Promise<void>;
      getAppIconDataUrl: () => Promise<string | null>;
      platform: string;
      onUpdateStatus: (cb: (status: unknown) => void) => void;
      installUpdateAndRestart: () => Promise<void>;
      openExternal: (url: string) => Promise<void>;
      windowMinimize: () => Promise<void>;
      windowMaximize: () => Promise<void>;
      windowClose: () => Promise<void>;
      getRefreshPaused: () => Promise<{ paused: boolean; pausedDueToFailures: boolean }>;
      setRefreshPaused: (paused: boolean) => Promise<void>;
      onPausedChanged: (cb: (state: { paused: boolean; pausedDueToFailures: boolean }) => void) => void;
      onAutoRefreshPausedForFailures: (cb: () => void) => void;
      // AI assistant. Declared here because this is the fullest `window.electron`
      // declaration in the renderer and the one TypeScript resolves against.
      aiChat: (payload: {
        messages: Array<{ role: 'user' | 'assistant'; content: string }>;
      }) => Promise<{ content: string; isError?: boolean }>;
      aiChatStream: (payload: {
        requestId: string;
        messages: Array<{ role: 'user' | 'assistant'; content: string }>;
      }) => Promise<{ content: string; isError?: boolean; aborted?: boolean }>;
      aiChatAbort: (requestId: string) => Promise<{ ok: boolean }>;
      onAiChatChunk: (cb: (payload: { requestId: string; delta: string }) => void) => () => void;
      // Identity Center. Only names and flags cross this boundary; tokens stay in main.
      ssoSignIn: (
        startUrl: string,
        region: string
      ) => Promise<{ success: true; identity?: string } | { success: false; error: string }>;
      ssoGetSessionStatus: (
        startUrl: string,
        region: string
      ) => Promise<{ signedIn: boolean; expiresAt?: string; identity?: string }>;
      ssoSignOut: (startUrl: string, region: string) => Promise<void>;
      ssoDetectRegion: (startUrl: string) => Promise<{ region: string } | { error: string }>;
      ssoListAccounts: (
        startUrl: string,
        region: string
      ) => Promise<{ accounts: SsoAccount[] } | { error: string }>;
      createProfiles: (profiles: Profile[]) => Promise<{ created: number }>;
      listBrowsers: () => Promise<{ key: string; name: string }[]>;
      exportOrgConfig: (
        organizationName?: string
      ) => Promise<{ canceled: true } | { success: true; path: string } | { success: false; error: string }>;
      importOrgConfig: () => Promise<
        { canceled: true } | { success: true; config: unknown } | { success: false; error: string }
      >;
      getAiModels: () => Promise<{ models: string[] } | { error: string }>;
      getMasterPasswordEnabled: () => Promise<boolean>;
      createMasterPassword: (
        password: string,
        confirmPassword: string
      ) => Promise<{ success: true } | { success: false; error: string }>;
      setDefaultCredentials: (
        username: string,
        password: string | null
      ) => Promise<void | { success: false; error: string }>;
      openAwsConsole: (
        profileId: string
      ) => Promise<{ success: true } | { success: false; error: string }>;
      onSsoLoginRequired: (
        cb: (profileId: string, startUrl: string, region: string) => void
      ) => () => void;
      refreshAutoRefreshProfiles: () => Promise<void>;
      onRefreshAllRequired: (
        cb: (credentialProfileIds: string[], defaultProfileIds: string[]) => void
      ) => void;
    };
  }
}

function roleToDisplayText(role: AwsRole, accountDisplayNames?: Record<string, string>): string {
  if (role.accountName?.trim()) {
    return role.accountName.trim();
  }
  const m = role.roleArn.match(/arn:aws:iam::(\d+):role\/(.+)/);
  if (!m) return role.displayText || role.roleArn;
  const [, accountId, roleName] = m;
  const displayName = accountDisplayNames?.[accountId]?.trim();
  if (displayName) {
    return `${displayName} (${accountId}) - ${roleName}`;
  }
  return `${accountId} / ${roleName}`;
}

// Inline SVG icons (24x24 viewBox, currentColor)
const IconRefresh = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
  </svg>
);
const IconPencil = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" />
  </svg>
);
const IconTrash = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
  </svg>
);
const IconPlus = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4" />
  </svg>
);
const IconCheck = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>
);
const IconX = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>
);
const IconClock = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>
);
const IconCloudLaunch = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.8} aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" d="M7 18a4 4 0 01-.6-7.955 5.5 5.5 0 0110.62-1.02A3.75 3.75 0 0117.5 18H7z" />
    <path strokeLinecap="round" strokeLinejoin="round" d="M12 21v-6m0 0l-2.25 2.25M12 15l2.25 2.25" />
  </svg>
);

const IconChevronDown = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2} aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
  </svg>
);

const IconGrip = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path d="M8 6a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0zm6-12a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0z" />
  </svg>
);
const IconList = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 6h16M4 10h16M4 14h16M4 18h16" />
  </svg>
);
const IconGrid = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6zM14 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6zM4 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2v-2zM14 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z" />
  </svg>
);
const IconFolder = ({
  className = 'w-4 h-4',
  style,
}: {
  className?: string;
  style?: React.CSSProperties;
}) => (
  <svg className={className} style={style} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.8} aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" d="M3 7a2 2 0 012-2h3.586a1 1 0 01.707.293l1.414 1.414a1 1 0 00.707.293H19a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2V7z" />
  </svg>
);
const IconFolderOut = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.8} aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" d="M3 7a2 2 0 012-2h3.586a1 1 0 01.707.293l1.414 1.414a1 1 0 00.707.293H19a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2V7z" />
    <path strokeLinecap="round" strokeLinejoin="round" d="M12 15V9m0 0l-2 2m2-2l2 2" />
  </svg>
);
const IconFolderPlus = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.8} aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" d="M3 7a2 2 0 012-2h3.586a1 1 0 01.707.293l1.414 1.414a1 1 0 00.707.293H19a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2V7z" />
    <path strokeLinecap="round" strokeLinejoin="round" d="M12 10v5m-2.5-2.5h5" />
  </svg>
);
const IconSearch = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
  </svg>
);

function formatTimeRemaining(seconds: number | undefined): string {
  if (seconds === undefined || seconds < 0) return '—';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return '< 1m';
}

const emptyProfile = (): Profile => ({
  id: uuidv4(),
  name: '',
  idpEntryUrl: '',
  label: '',
  autoRefresh: false,
  refreshIntervalMinutes: 60,
  useDefaultCredentials: false,
  credentialProfileName: '',
  authType: 'saml',
});

/** "123456789012 / RegionalAdmin", or the friendly account name when one is configured. */
function ssoRoleDisplayText(
  account: SsoAccount,
  roleName: string,
  accountDisplayNames?: Record<string, string>
): string {
  const friendly = accountDisplayNames?.[account.accountId]?.trim() || account.accountName?.trim();
  return friendly
    ? `${friendly} (${account.accountId}) - ${roleName}`
    : `${account.accountId} / ${roleName}`;
}

const StatusBadge = ({ status }: { status: DashboardProfileSummary['status'] }) => {
  if (status === 'active')
    return (
      <span className="inline-flex items-center gap-1.5 rounded-full bg-discord-success/20 px-2.5 py-0.5 text-xs font-medium text-discord-success">
        <IconCheck className="w-3.5 h-3.5" />
        Active
      </span>
    );
  if (status === 'expired')
    return (
      <span className="inline-flex items-center gap-1.5 rounded-full bg-discord-danger/20 px-2.5 py-0.5 text-xs font-medium text-discord-danger">
        <IconX className="w-3.5 h-3.5" />
        Expired
      </span>
    );
  return (
    <span className="inline-flex items-center gap-1.5 rounded-full bg-discord-darkest px-2.5 py-0.5 text-xs font-medium text-discord-textMuted">
      <IconClock className="w-3.5 h-3.5" />
      Never
    </span>
  );
};

interface ProfileCardActions {
  onOpenConsole: (id: string) => void;
  onRefresh: (id: string) => void;
  onEdit: (p: DashboardProfileSummary) => void;
  onDelete: (p: DashboardProfileSummary) => void;
  onMoveToFolder: (profileId: string, folderId: string | null) => void;
}

/** "Move to folder…" — the non-drag path, and the one that works without a pointer. */
function MoveToFolderMenu({
  profile,
  folders,
  onMoveToFolder,
}: {
  profile: DashboardProfileSummary;
  folders: ProfileFolder[];
  onMoveToFolder: (profileId: string, folderId: string | null) => void;
}) {
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);

  const currentFolderId = folders.some((f) => f.id === profile.folderId) ? profile.folderId : undefined;

  return (
    <div>
      <Tooltip label="Move to folder" placement="above">
        <button
          ref={buttonRef}
          type="button"
          onClick={() => setOpen((o) => !o)}
          className="rounded-button p-2 text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
          aria-label={`Move ${profile.name} to folder`}
          aria-haspopup="menu"
          aria-expanded={open}
        >
          <IconFolder className="w-4 h-4" />
        </button>
      </Tooltip>
      {open && (
        <FloatingMenu
          {...menuAnchorFor(buttonRef.current)}
          align="right"
          triggerRef={buttonRef}
          onClose={() => setOpen(false)}
          className="max-h-64 w-56 overflow-y-auto"
        >
          {folders.length === 0 && (
            <div className="px-3 py-2 text-xs text-discord-textMuted">No folders yet</div>
          )}
          {folders.map((f) => (
            <button
              key={f.id}
              type="button"
              role="menuitem"
              disabled={f.id === currentFolderId}
              onClick={() => {
                setOpen(false);
                onMoveToFolder(profile.id, f.id);
              }}
              className="flex w-full items-center gap-2 rounded-button px-3 py-2 text-left text-sm text-discord-text hover:bg-discord-darkest transition-colors disabled:opacity-40"
            >
              <IconFolder className="w-4 h-4 flex-shrink-0" />
              <span className="truncate">{f.name}</span>
            </button>
          ))}
          {currentFolderId && (
            <>
              <div className="my-1 h-px bg-discord-border" role="separator" />
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setOpen(false);
                  onMoveToFolder(profile.id, null);
                }}
                className="w-full rounded-button px-3 py-2 text-left text-sm text-discord-text hover:bg-discord-darkest transition-colors"
              >
                Remove from folder
              </button>
            </>
          )}
        </FloatingMenu>
      )}
    </div>
  );
}

/**
 * Presentational profile card. Split from the sortable wrapper so the DragOverlay can render an
 * identical copy without registering a second sortable under the same id.
 */
function ProfileCardBody({
  p,
  viewMode,
  folders,
  handle,
  refreshing,
  refreshDisabled,
  openingConsole,
  actions,
}: {
  p: DashboardProfileSummary;
  viewMode: 'list' | 'grid';
  folders: ProfileFolder[];
  handle: React.ReactNode;
  refreshing: boolean;
  refreshDisabled: boolean;
  openingConsole: boolean;
  actions: ProfileCardActions;
}) {
  const actionButtons = (
    <>
      <Tooltip label="Open AWS console" placement="above">
        <button
          onClick={() => actions.onOpenConsole(p.id)}
          disabled={openingConsole}
          className="rounded-button p-2 text-discord-textMuted hover:bg-discord-accent hover:text-white transition-colors disabled:opacity-50"
        >
          <IconCloudLaunch className={`w-4 h-4 ${openingConsole ? 'animate-pulse' : ''}`} />
        </button>
      </Tooltip>
      <Tooltip label="Refresh credentials" placement="above">
        <button
          onClick={() => actions.onRefresh(p.id)}
          disabled={refreshDisabled}
          className="rounded-button p-2 text-discord-textMuted hover:bg-discord-accent hover:text-white transition-colors disabled:opacity-50"
        >
          <IconRefresh className={`w-4 h-4 ${refreshing ? 'animate-spin' : ''}`} />
        </button>
      </Tooltip>
      <MoveToFolderMenu profile={p} folders={folders} onMoveToFolder={actions.onMoveToFolder} />
      <Tooltip label="Edit profile" placement="above">
        <button
          onClick={() => actions.onEdit(p)}
          className="rounded-button p-2 text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
        >
          <IconPencil className="w-4 h-4" />
        </button>
      </Tooltip>
      <Tooltip label="Delete profile" placement="above" align="right">
        <button
          onClick={() => actions.onDelete(p)}
          className="rounded-button p-2 text-discord-textMuted hover:bg-discord-danger/20 hover:text-discord-danger transition-colors"
        >
          <IconTrash className="w-4 h-4" />
        </button>
      </Tooltip>
    </>
  );

  if (viewMode === 'list') {
    return (
      <>
        {handle}
        <ProfileAvatar iconName={p.iconName} iconColor={p.iconColor} />
        <div className="flex-1 min-w-0 py-3">
          <div className="font-semibold text-discord-text truncate">{p.name}</div>
          <div className="text-sm text-discord-textMuted truncate mt-0.5">{p.accountNumber}</div>
          <div className="flex flex-wrap items-center gap-3 mt-2">
            <StatusBadge status={p.status} />
            <span className="text-xs text-discord-textMuted">
              {formatTimeRemaining(p.timeRemainingSeconds)} left
            </span>
            {p.expiresAtPst && (
              <span className="text-xs text-discord-textMuted">
                {p.status === 'expired' ? 'Expired' : 'Expires'} {p.expiresAtPst}
              </span>
            )}
          </div>
        </div>
        <div className="flex-shrink-0 flex items-center gap-1 py-3 pr-2">{actionButtons}</div>
      </>
    );
  }

  return (
    <>
      <div className="flex items-start gap-3 p-4">
        {handle}
        <ProfileAvatar
          iconName={p.iconName}
          iconColor={p.iconColor}
          className="flex-shrink-0 w-14 h-14 rounded-xl bg-discord-panel border border-discord-border flex items-center justify-center overflow-hidden"
          iconClassName="w-7 h-7"
        />
        <div className="flex-1 min-w-0">
          <div className="font-semibold text-discord-text truncate">{p.name}</div>
          <div className="text-sm text-discord-textMuted truncate mt-0.5">{p.accountNumber}</div>
          <div className="mt-2">
            <StatusBadge status={p.status} />
          </div>
        </div>
      </div>
      <div className="px-4 pb-4 pt-0 flex flex-wrap items-center justify-between gap-2 border-t border-discord-border/50 mt-auto">
        <div className="text-xs text-discord-textMuted">
          <span>{formatTimeRemaining(p.timeRemainingSeconds)} left</span>
          {p.expiresAtPst && (
            <span className="ml-2">
              · {p.status === 'expired' ? 'Expired' : 'Expires'} {p.expiresAtPst}
            </span>
          )}
        </div>
        <div className="flex items-center gap-1">{actionButtons}</div>
      </div>
    </>
  );
}

/**
 * Right-click menu for moving a profile between folders.
 *
 * The drag path is fine for neighbours but poor across a long list — dragging a profile from the
 * bottom of the page into a folder at the top means dragging through everything in between. This
 * is the distance-independent path, and it works the same whether the profile is in a folder or
 * not.
 *
 * Rendered through a portal at fixed coordinates: the row it belongs to sits inside scrolling,
 * overflow-hidden containers that would otherwise clip it.
 */
function ProfileContextMenu({
  x,
  y,
  profile,
  folders,
  onMove,
  onClose,
}: {
  x: number;
  y: number;
  profile: DashboardProfileSummary;
  folders: ProfileFolder[];
  onMove: (profileId: string, folderId: string | null) => void;
  onClose: () => void;
}) {
  const currentFolderId = folders.some((f) => f.id === profile.folderId)
    ? profile.folderId
    : undefined;

  return (
    <FloatingMenu x={x} y={y} onClose={onClose} className="w-60">
      <div className="truncate px-3 py-2 text-xs font-semibold uppercase tracking-wide text-discord-textMuted">
        {profile.name}
      </div>

      {currentFolderId && (
        <>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onMove(profile.id, null);
              onClose();
            }}
            className="flex w-full items-center gap-2 rounded-button px-3 py-2 text-left text-sm text-discord-text hover:bg-discord-darkest transition-colors"
          >
            <IconFolderOut className="w-4 h-4 flex-shrink-0" />
            Move out of folder
          </button>
          <div className="my-1 h-px bg-discord-border" role="separator" />
        </>
      )}

      {folders.length === 0 ? (
        <div className="px-3 py-2 text-xs text-discord-textMuted">
          No folders yet — create one with the folder button above.
        </div>
      ) : (
        <>
          <div className="px-3 pb-1 pt-1 text-xs text-discord-textMuted">Move to folder</div>
          <div className="max-h-[50vh] overflow-y-auto">
            {folders.map((f) => (
              <button
                key={f.id}
                type="button"
                role="menuitem"
                disabled={f.id === currentFolderId}
                onClick={() => {
                  onMove(profile.id, f.id);
                  onClose();
                }}
                className="flex w-full items-center gap-2 rounded-button px-3 py-2 text-left text-sm text-discord-text hover:bg-discord-darkest transition-colors disabled:opacity-40 disabled:hover:bg-transparent"
              >
                <IconFolder
                  className="w-4 h-4 flex-shrink-0"
                  style={f.color ? { color: f.color } : undefined}
                />
                <span className="truncate">{f.name}</span>
                {f.id === currentFolderId && (
                  <span className="ml-auto flex-shrink-0 text-xs text-discord-textMuted">current</span>
                )}
              </button>
            ))}
          </div>
        </>
      )}
    </FloatingMenu>
  );
}

/**
 * Drop target for "no folder". Always rendered (even when empty) so there is somewhere to drag a
 * profile back out of a folder to.
 */
function UngroupedDropZone({
  showHeading,
  children,
}: {
  showHeading: boolean;
  children: React.ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: UNGROUPED_CONTAINER_ID });
  return (
    <div
      ref={setNodeRef}
      className={`rounded-card transition-colors ${
        isOver ? 'bg-discord-accent/5 ring-1 ring-discord-accent' : ''
      }`}
    >
      {showHeading && (
        <div className="flex items-center gap-2 px-2 pt-2 pb-1">
          <span className="text-xs font-semibold uppercase tracking-wide text-discord-textMuted">
            Ungrouped
          </span>
          <span className="h-px flex-1 bg-discord-border" />
        </div>
      )}
      <div className={showHeading ? 'px-2 pb-2' : ''}>{children}</div>
    </div>
  );
}

const cardShellClass = (viewMode: 'list' | 'grid') =>
  viewMode === 'list'
    ? 'flex items-center gap-4 rounded-card border bg-discord-darkest/50 transition-all'
    : 'flex flex-col rounded-card border bg-discord-darkest/50 transition-all';

function SortableProfileCard({
  onContextMenu,
  ...props
}: {
  p: DashboardProfileSummary;
  viewMode: 'list' | 'grid';
  folders: ProfileFolder[];
  refreshing: boolean;
  refreshDisabled: boolean;
  openingConsole: boolean;
  actions: ProfileCardActions;
  onContextMenu: (e: React.MouseEvent, p: DashboardProfileSummary) => void;
}) {
  // Dragging stays enabled while a search is active: the drop handler rebuilds the order from the
  // full unfiltered list, so hidden profiles keep their positions and a filtered drag is exactly
  // how you bulk-move a matching set into a folder.
  const { p, viewMode } = props;
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: `profile:${p.id}`,
  });

  const handle = (
    <Tooltip label="Drag to reorder or move into a folder" placement="above" align="left">
      <span
        className={`flex-shrink-0 ${viewMode === 'list' ? 'p-2' : 'p-1.5'} cursor-grab text-discord-textMuted hover:text-discord-text active:cursor-grabbing rounded`}
        {...listeners}
      >
        <IconGrip className="w-4 h-4" />
      </span>
    </Tooltip>
  );

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={`${cardShellClass(viewMode)} ${
        isDragging ? 'opacity-40' : 'border-discord-border hover:border-discord-borderLight hover:bg-discord-panelHover/50'
      }`}
      // dnd-kit's PointerSensor ignores the right button, so this never competes with a drag.
      onContextMenu={(e) => onContextMenu(e, p)}
      {...attributes}
    >
      <ProfileCardBody {...props} handle={handle} />
    </div>
  );
}

export default function Profiles() {
  const [dashboardProfiles, setDashboardProfiles] = useState<DashboardProfileSummary[]>([]);
  const [editing, setEditing] = useState<Profile | null>(null);
  const [form, setForm] = useState<Profile>(emptyProfile());
  const [refreshingIds, setRefreshingIds] = useState<Set<string>>(new Set());
  const [credentialsModal, setCredentialsModal] = useState<string | null>(null);
  const [credentialsPrefillUsername, setCredentialsPrefillUsername] = useState('');
  const [roleModal, setRoleModal] = useState<{ profileId: string; roles: AwsRole[]; profileName: string } | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const [networkUnavailable, setNetworkUnavailable] = useState(false);
  const [rolesForIdp, setRolesForIdp] = useState<AwsRole[] | null>(null);
  const [loadingRoles, setLoadingRoles] = useState(false);
  const [fetchRolesModal, setFetchRolesModal] = useState<{ idpEntryUrl: string; prefillUsername?: string } | null>(null);
  const [accountDisplayNames, setAccountDisplayNames] = useState<Record<string, string>>({});
  const [folders, setFolders] = useState<ProfileFolder[]>([]);
  const [collapsedFolderIds, setCollapsedFolderIds] = useState<string[]>([]);
  /** The dnd-kit id currently being dragged ('profile:x' / 'folder:y'), for the DragOverlay. */
  const [activeDragId, setActiveDragId] = useState<string | null>(null);
  const [folderDeleteConfirm, setFolderDeleteConfirm] = useState<{
    id: string;
    name: string;
    count: number;
  } | null>(null);
  /** Open right-click menu: which profile, and where the pointer was. */
  const [contextMenu, setContextMenu] = useState<{ profileId: string; x: number; y: number } | null>(
    null
  );
  const [deleteConfirm, setDeleteConfirm] = useState<{ id: string; name: string } | null>(null);
  const [refreshAllModal, setRefreshAllModal] = useState<{
    credentialProfileIds: string[];
    defaultProfileIds: string[];
  } | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [viewMode, setViewMode] = useState<'list' | 'grid'>('list');
  // Identity Center
  const [ssoAccounts, setSsoAccounts] = useState<SsoAccount[] | null>(null);
  const [ssoBusy, setSsoBusy] = useState(false);
  const [ssoIdentity, setSsoIdentity] = useState<string | null>(null);
  const [ssoLoginNotice, setSsoLoginNotice] = useState<{ startUrl: string; region: string } | null>(null);
  /** Settings defaults, kept so switching a profile to Identity Center can prefill the org. */
  const [ssoDefaults, setSsoDefaults] = useState<{ startUrl: string; region: string }>({ startUrl: '', region: '' });
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const [openingConsoleIds, setOpeningConsoleIds] = useState<Set<string>>(new Set());
  const [refreshPauseState, setRefreshPauseState] = useState({
    paused: false,
    pausedDueToFailures: false,
  });
  const [autoRefreshFailureModalOpen, setAutoRefreshFailureModalOpen] = useState(false);

  /**
   * True from the moment a drag starts until its write has landed.
   *
   * The 10s poll below would otherwise replace the optimistic order with the server's not-yet-
   * updated one and snap rows back under the cursor mid-drag.
   */
  const layoutBusyRef = useRef(false);

  const load = useCallback(async () => {
    const [summaries, folderList] = await Promise.all([
      window.electron.getDashboardState(),
      window.electron.getFolders(),
    ]);
    if (layoutBusyRef.current) return;
    setDashboardProfiles(summaries);
    setFolders(folderList);
  }, []);

  const filteredProfiles = dashboardProfiles.filter((p) => {
    if (!searchQuery.trim()) return true;
    const q = searchQuery.trim().toLowerCase();
    return (
      p.name.toLowerCase().includes(q) ||
      (p.label && p.label.toLowerCase().includes(q)) ||
      (p.accountNumber && p.accountNumber.toLowerCase().includes(q))
    );
  });

  useEffect(() => {
    void load();
    const interval = setInterval(() => void load(), 10000);
    return () => clearInterval(interval);
  }, [load]);

  useEffect(() => {
    window.electron.getCollapsedFolders().then(setCollapsedFolderIds);
  }, []);

  useEffect(() => {
    window.electron.getRefreshPaused().then(setRefreshPauseState);
  }, []);

  useEffect(() => {
    window.electron.onPausedChanged(setRefreshPauseState);
    window.electron.onAutoRefreshPausedForFailures(() => setAutoRefreshFailureModalOpen(true));
  }, []);

  useEffect(() => {
    window.electron.getSettings().then((s) => setAccountDisplayNames(s?.accountDisplayNames ?? {}));
  }, []);

  useEffect(() => {
    if (editing && form.idpEntryUrl) {
      window.electron.getCachedRoles(form.idpEntryUrl).then((r) => setRolesForIdp(r ?? null));
    }
  }, [form.idpEntryUrl, editing]);

  useEffect(() => {
    window.electron.onCredentialsRequired((profileId, prefillUsername) => {
      setCredentialsModal(profileId);
      setCredentialsPrefillUsername(prefillUsername ?? '');
      setLastError(null);
    });
    window.electron.onCredentialsRefreshed((profileId) => {
      setRefreshingIds((s) => { const n = new Set(s); n.delete(profileId); return n; });
      setCredentialsModal((current) => (current === profileId ? null : current));
      setLastError(null);
      setNetworkUnavailable(false);
      load();
    });
    window.electron.onRefreshStarted((profileId) => {
      setRefreshingIds((s) => new Set(s).add(profileId));
      // A new attempt is in flight; clear stale errors from previous attempts.
      setLastError(null);
      setNetworkUnavailable(false);
    });
    window.electron.onCredentialsExpired((profileId, message) => {
      setRefreshingIds((s) => { const n = new Set(s); n.delete(profileId); return n; });
      setLastError(message);
      load();
    });
    window.electron.onNetworkUnavailable?.((profileId) => {
      setRefreshingIds((s) => { const n = new Set(s); n.delete(profileId); return n; });
      setNetworkUnavailable(true);
    });
    window.electron.onRefreshAllRequired?.((credentialProfileIds: string[], defaultProfileIds: string[]) => {
      setRefreshAllModal({ credentialProfileIds, defaultProfileIds });
      setLastError(null);
    });
    // The wizard creates profiles outside this page's state; reload when it says so.
    const onProfilesChanged = () => load();
    window.addEventListener('profiles:changed', onProfilesChanged);
    // Scheduler found an org whose SSO session needs a human. Offer a sign-in; never auto-open one.
    window.electron.onSsoLoginRequired?.((profileId, startUrl, region) => {
      setRefreshingIds((s) => { const n = new Set(s); n.delete(profileId); return n; });
      setSsoLoginNotice({ startUrl, region });
    });
  }, []);

  // Dismiss the add-profile menu on outside click or Escape.
  useEffect(() => {
    if (!addMenuOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      if (!(e.target as HTMLElement)?.closest('[data-add-menu]')) setAddMenuOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setAddMenuOpen(false);
    };
    document.addEventListener('mousedown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [addMenuOpen]);

  // Belt-and-suspenders: clear the offline banner the moment the OS reports we're back online.
  // The next scheduled or manual refresh will confirm; if it fails for a real auth reason we'll show that instead.
  useEffect(() => {
    const onOnline = () => setNetworkUnavailable(false);
    window.addEventListener('online', onOnline);
    return () => window.removeEventListener('online', onOnline);
  }, []);

  const startAdd = async () => {
    const newProfile = emptyProfile();
    const [settings, defaultCreds] = await Promise.all([
      window.electron.getSettings(),
      window.electron.getDefaultCredentialsDisplay(),
    ]);
    setAccountDisplayNames(settings?.accountDisplayNames ?? {});
    const idpUrl = settings?.defaultIdpEntryUrl ?? '';
    const ssoStartUrl = settings?.defaultSsoStartUrl ?? '';
    const ssoRegion = settings?.defaultSsoRegion ?? '';
    setSsoDefaults({ startUrl: ssoStartUrl, region: ssoRegion });
    const useDefault = !!defaultCreds;
    const defaultHours = settings?.defaultSessionDurationHours ?? 1;
    const refreshMinutes = Math.max(60, Math.floor(defaultHours * 60));
    setForm({
      ...newProfile,
      idpEntryUrl: idpUrl,
      // Prefilled for both types: the SAML fields are hidden while authType is 'saml', so this
      // just means switching to Identity Center already has the org filled in.
      ssoStartUrl,
      ssoRegion,
      useDefaultCredentials: useDefault,
      refreshIntervalMinutes: refreshMinutes,
    });
    setEditing(newProfile);
    setRolesForIdp(null);
    if (idpUrl) {
      const cached = await window.electron.getCachedRoles(idpUrl);
      setRolesForIdp(cached ?? null);
    }
  };

  const startEdit = async (p: DashboardProfileSummary) => {
    const [full, settings] = await Promise.all([
      window.electron.getProfileById(p.id),
      window.electron.getSettings(),
    ]);
    if (full) {
      setAccountDisplayNames(settings?.accountDisplayNames ?? {});
      setSsoDefaults({
        startUrl: settings?.defaultSsoStartUrl ?? '',
        region: settings?.defaultSsoRegion ?? '',
      });
      setForm({ ...full });
      setEditing(full);
      if (full.idpEntryUrl) {
        const cached = await window.electron.getCachedRoles(full.idpEntryUrl);
        setRolesForIdp(cached ?? null);
      } else {
        setRolesForIdp(null);
      }
    }
  };

  const formAuthType = resolveAuthType(form);
  const isSsoForm = formAuthType === 'identityCenter';

  const commonFieldsValid = !!form.name?.trim() && !!form.credentialProfileName?.trim();
  const requiredFieldsValid = isSsoForm
    ? commonFieldsValid &&
      !!form.ssoStartUrl?.trim() &&
      !!form.ssoRegion?.trim() &&
      !!form.ssoAccountId?.trim() &&
      !!form.ssoRoleName?.trim()
    : commonFieldsValid && !!form.idpEntryUrl?.trim() && !!form.roleArn?.trim();

  const save = async () => {
    if (!requiredFieldsValid) {
      setLastError(
        isSsoForm
          ? 'Please fill in all required fields: Profile name, Credentials section name, SSO start URL, SSO region, and Account / Role.'
          : 'Please fill in all required fields: Profile name, Credentials section name, IdP entry URL, and Role / Account.'
      );
      return;
    }
    setLastError(null);
    const toSave = {
      ...form,
      credentialProfileName: form.credentialProfileName || form.name,
      // Normalize on save so a pasted '.../start/#/' can't reach the API layer.
      ...(isSsoForm ? { ssoStartUrl: normalizeStartUrl(form.ssoStartUrl ?? '') } : {}),
    };
    await window.electron.saveProfile(toSave);
    setEditing(null);
    setFetchRolesModal(null);
    load();
  };

  const remove = async (id: string) => {
    await window.electron.deleteProfile(id);
    if (editing?.id === id) setEditing(null);
    setDeleteConfirm(null);
    load();
  };

  const handleRefresh = async (id: string) => {
    setRefreshingIds((s) => new Set(s).add(id));
    setLastError(null);
    try {
      const result = await window.electron.refreshProfile(id);
      const r = result as {
        required?: boolean;
        profileId?: string;
        prefillUsername?: string;
        success?: boolean;
        error?: string;
        roles?: AwsRole[];
        ssoLoginRequired?: boolean;
        startUrl?: string;
        region?: string;
      };
      if (r.ssoLoginRequired && r.startUrl && r.region) {
        // Not an error: the SSO session needs a human. Offer the sign-in rather than shouting.
        setSsoLoginNotice({ startUrl: r.startUrl, region: r.region });
      } else if (r.required && r.profileId) {
        setCredentialsModal(r.profileId);
        setCredentialsPrefillUsername(r.prefillUsername ?? '');
      } else if (r.roles && r.profileId) {
        const name = dashboardProfiles.find((p) => p.id === r.profileId)?.name ?? 'Profile';
        setRoleModal({ profileId: r.profileId, roles: r.roles, profileName: name });
      } else if (r.success === true) {
        setRefreshingIds((s) => { const n = new Set(s); n.delete(id); return n; });
        load();
      } else if (r.success === false && r.error) {
        setLastError(r.error);
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setLastError(message);
    } finally {
      setRefreshingIds((s) => { const n = new Set(s); n.delete(id); return n; });
    }
  };

  /** Sign in if needed, then load the account/role list for the form's org. */
  const handleLoadSsoAccounts = async () => {
    const startUrl = normalizeStartUrl(form.ssoStartUrl ?? '');
    const region = (form.ssoRegion ?? '').trim();
    if (!startUrl || !region) {
      setLastError('Set the SSO start URL and region first.');
      return;
    }
    setSsoBusy(true);
    setLastError(null);
    try {
      const result = await window.electron.ssoListAccounts(startUrl, region);
      if ('error' in result) {
        setLastError(result.error);
        return;
      }
      setSsoAccounts(result.accounts);
      const status = await window.electron.ssoGetSessionStatus(startUrl, region);
      setSsoIdentity(status.identity ?? null);
    } catch (err) {
      setLastError(err instanceof Error ? err.message : String(err));
    } finally {
      setSsoBusy(false);
    }
  };

  const handleDetectSsoRegion = async () => {
    const startUrl = normalizeStartUrl(form.ssoStartUrl ?? '');
    if (!startUrl) {
      setLastError('Enter the SSO start URL first.');
      return;
    }
    setSsoBusy(true);
    setLastError(null);
    try {
      const result = await window.electron.ssoDetectRegion(startUrl);
      if ('error' in result) setLastError(result.error);
      else setForm((f) => ({ ...f, ssoRegion: result.region }));
    } finally {
      setSsoBusy(false);
    }
  };

  /** One-click AWS console. Main refreshes stale credentials first, so this can take a moment. */
  const handleOpenConsole = async (id: string) => {
    setOpeningConsoleIds((s) => new Set(s).add(id));
    setLastError(null);
    try {
      const result = await window.electron.openAwsConsole(id);
      if (!result.success) setLastError(result.error);
      else load();
    } catch (err) {
      setLastError(err instanceof Error ? err.message : String(err));
    } finally {
      setOpeningConsoleIds((s) => { const n = new Set(s); n.delete(id); return n; });
    }
  };

  const handleRefreshRoles = async () => {
    if (!form.idpEntryUrl?.trim()) {
      setLastError('Set IdP entry URL first, then load roles.');
      return;
    }
    setLoadingRoles(true);
    setLastError(null);
    try {
      const result = await window.electron.fetchRoles(
        form.idpEntryUrl,
        form.useDefaultCredentials ?? false,
        editing?.id
      ) as { roles?: AwsRole[]; credentialsRequired?: boolean; prefillUsername?: string; success?: boolean; error?: string };
      if (result.roles) {
        setRolesForIdp(result.roles);
      } else if (result.credentialsRequired) {
        setFetchRolesModal({ idpEntryUrl: form.idpEntryUrl, prefillUsername: result.prefillUsername });
      } else if (result.success === false && result.error) {
        setLastError(result.error);
      }
    } catch (err) {
      setLastError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoadingRoles(false);
    }
  };

  /**
   * Drag & drop.
   *
   * Two kinds of sortable item share one DndContext: profiles ('profile:<id>') and folders
   * ('folder:<id>'). Folder sections are also droppable containers ('container:<folderId>'), as is
   * the ungrouped area, so a profile can be dropped onto a folder as a whole rather than onto a
   * specific row — which is the only way to target a collapsed or empty folder.
   *
   * Moves are applied to local state first and persisted in the background. The previous
   * implementation waited on the IPC round trip before the row moved, which reads as lag.
   */
  const sensors = useSensors(
    // A few pixels of travel before a drag begins, so clicking the refresh/edit/delete buttons
    // inside a draggable row never turns into a drag.
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  );

  const collapsedSet = useMemo(() => new Set(collapsedFolderIds), [collapsedFolderIds]);

  const toggleFolderCollapsed = useCallback((folderId: string) => {
    setCollapsedFolderIds((prev) => {
      const next = prev.includes(folderId)
        ? prev.filter((id) => id !== folderId)
        : [...prev, folderId];
      void window.electron.setCollapsedFolders(next);
      return next;
    });
  }, []);

  /** Persist a profile's new position and folder. Only the moved profile can change folder. */
  const persistLayout = (ordered: DashboardProfileSummary[], movedId: string, folderId: string | null) => {
    layoutBusyRef.current = true;
    window.electron
      .applyProfileLayout(
        ordered.map((p) => p.id),
        { [movedId]: folderId }
      )
      .catch((err: unknown) => setLastError(err instanceof Error ? err.message : String(err)))
      .finally(() => {
        layoutBusyRef.current = false;
        void load();
      });
  };

  /**
   * Which container a dnd-kit `over` id belongs to, as a folder id (null = ungrouped,
   * undefined = not a drop target we understand).
   *
   * All three id shapes have to resolve here. A folder section is registered twice on the same
   * element — as a sortable ('folder:<id>') and as a droppable container ('container:<id>') — so
   * collision detection legitimately returns either one depending on where the pointer is, and a
   * drop on a row reports that row's id instead.
   */
  const containerFolderIdOf = (overId: string): string | null | undefined => {
    if (overId === UNGROUPED_CONTAINER_ID) return null;
    if (overId.startsWith(FOLDER_CONTAINER_PREFIX)) {
      return overId.slice(FOLDER_CONTAINER_PREFIX.length);
    }
    if (overId.startsWith(FOLDER_SORTABLE_PREFIX)) {
      return overId.slice(FOLDER_SORTABLE_PREFIX.length);
    }
    if (overId.startsWith('profile:')) {
      const target = dashboardProfiles.find((p) => `profile:${p.id}` === overId);
      if (!target) return undefined;
      // A folderId naming a folder that no longer exists means ungrouped, same as the renderer.
      return target.folderId && folders.some((f) => f.id === target.folderId)
        ? target.folderId
        : null;
    }
    return undefined;
  };

  const handleDragStart = (event: DragStartEvent) => {
    layoutBusyRef.current = true;
    setActiveDragId(String(event.active.id));
  };

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;
    setActiveDragId(null);
    const activeId = String(active.id);

    if (!over) {
      layoutBusyRef.current = false;
      return;
    }
    const overId = String(over.id);

    // Folder reorder
    if (activeId.startsWith(FOLDER_SORTABLE_PREFIX)) {
      layoutBusyRef.current = false;
      const fromId = activeId.slice(FOLDER_SORTABLE_PREFIX.length);
      /**
       * A folder section carries TWO dnd ids on one element — 'folder:<id>' (sortable) and
       * 'container:<id>' (droppable) — so collision detection can hand back either, and a drop on
       * a row inside a folder reports that row. Resolve all three to the folder being dropped on,
       * or folder reordering silently no-ops depending on where the pointer landed.
       */
      const toId = overId.startsWith(FOLDER_SORTABLE_PREFIX)
        ? overId.slice(FOLDER_SORTABLE_PREFIX.length)
        : (containerFolderIdOf(overId) ?? null);
      if (!toId || toId === fromId) return;
      const fromIdx = folders.findIndex((f) => f.id === fromId);
      const toIdx = folders.findIndex((f) => f.id === toId);
      if (fromIdx === -1 || toIdx === -1) return;
      const next = [...folders];
      next.splice(toIdx, 0, next.splice(fromIdx, 1)[0]);
      setFolders(next);
      void window.electron.reorderFolders(next.map((f) => f.id)).then(() => load());
      return;
    }

    // Profile move / reorder
    const profileId = activeId.slice('profile:'.length);
    const targetFolderId = containerFolderIdOf(overId);
    if (targetFolderId === undefined) {
      layoutBusyRef.current = false;
      return;
    }

    const moved = dashboardProfiles.find((p) => p.id === profileId);
    if (!moved) {
      layoutBusyRef.current = false;
      return;
    }

    // Rebuild through the grouping rather than splicing the flat array: that keeps each folder's
    // members contiguous, which is what makes the flat order round-trip through storage intact.
    const grouped = groupProfilesByFolder(dashboardProfiles, folders);
    const bucketOf = (folderId: string | null) =>
      folderId === null
        ? grouped.ungrouped
        : (grouped.groups.find((g) => g.folder.id === folderId)?.profiles ?? grouped.ungrouped);

    const destination = bucketOf(targetFolderId);
    /**
     * Index of the row being dropped on, taken BEFORE the dragged row is removed — this is
     * arrayMove's contract, and getting it wrong lands every downward drag one slot short (drag
     * a onto c in [a,b,c] and you get [b,a,c] instead of [b,c,a]). Cross-folder moves are
     * unaffected either way, since the dragged row isn't in the destination to begin with.
     */
    const insertAt = overId.startsWith('profile:')
      ? destination.findIndex((p) => `profile:${p.id}` === overId)
      : -1;

    for (const bucket of [grouped.ungrouped, ...grouped.groups.map((g) => g.profiles)]) {
      const idx = bucket.findIndex((p) => p.id === profileId);
      if (idx >= 0) bucket.splice(idx, 1);
    }

    const updated = { ...moved, folderId: targetFolderId ?? undefined };
    if (insertAt >= 0) destination.splice(insertAt, 0, updated);
    else destination.push(updated);

    const ordered = flattenGrouping(grouped);
    setDashboardProfiles(ordered);
    persistLayout(ordered, profileId, targetFolderId);
  };

  const handleDragCancel = () => {
    setActiveDragId(null);
    layoutBusyRef.current = false;
  };

  const handleCreateFolder = async () => {
    const created = await window.electron.saveFolder({ name: 'New folder' });
    // Matches folderStorage's prepend, so the optimistic update and the reload agree on position.
    setFolders((prev) => [created, ...prev]);
    await load();
  };

  const handleRenameFolder = async (id: string, name: string) => {
    const saved = await window.electron.saveFolder({ id, name });
    setFolders((prev) => prev.map((f) => (f.id === id ? saved : f)));
  };

  const handleRecolorFolder = async (id: string, color: string) => {
    const saved = await window.electron.saveFolder({ id, color });
    setFolders((prev) => prev.map((f) => (f.id === id ? saved : f)));
  };

  /** Deleting a folder never deletes its profiles — they move to Ungrouped. */
  const handleDeleteFolder = async (id: string) => {
    await window.electron.deleteFolder(id);
    setFolderDeleteConfirm(null);
    await load();
  };

  const handleProfileContextMenu = useCallback((e: React.MouseEvent, p: DashboardProfileSummary) => {
    e.preventDefault();
    setContextMenu({ profileId: p.id, x: e.clientX, y: e.clientY });
  }, []);

  const closeContextMenu = useCallback(() => setContextMenu(null), []);

  /** The non-drag path: also the keyboard-accessible way to move a profile. */
  const handleMoveToFolder = async (profileId: string, folderId: string | null) => {
    const grouped = groupProfilesByFolder(dashboardProfiles, folders);
    const moved = dashboardProfiles.find((p) => p.id === profileId);
    if (!moved) return;
    for (const bucket of [grouped.ungrouped, ...grouped.groups.map((g) => g.profiles)]) {
      const idx = bucket.findIndex((p) => p.id === profileId);
      if (idx >= 0) bucket.splice(idx, 1);
    }
    const destination =
      folderId === null
        ? grouped.ungrouped
        : (grouped.groups.find((g) => g.folder.id === folderId)?.profiles ?? grouped.ungrouped);
    destination.push({ ...moved, folderId: folderId ?? undefined });
    const ordered = flattenGrouping(grouped);
    setDashboardProfiles(ordered);
    persistLayout(ordered, profileId, folderId);
  };

  const handleFetchRolesSubmit = async (username: string, password: string) => {
    if (!fetchRolesModal) return;
    setLastError(null);
    try {
      const result = await window.electron.fetchRolesWithCredentials(
        fetchRolesModal.idpEntryUrl,
        username,
        password
      ) as { roles?: AwsRole[]; success?: boolean; error?: string };
      if (result.roles) {
        setRolesForIdp(result.roles);
        setFetchRolesModal(null);
      } else if (result.error) {
        setLastError(result.error);
      }
    } catch (err) {
      setLastError(err instanceof Error ? err.message : String(err));
    }
  };

  const handleSubmitCredentials = async (profileId: string, username: string, password: string) => {
    try {
      const result = await window.electron.submitCredentials(profileId, username, password);
      const r = result as {
        success?: boolean;
        error?: string;
        required?: boolean;
        roles?: AwsRole[];
        profileId?: string;
        prefillUsername?: string;
      };
      if (r.success === true) {
        setCredentialsModal(null);
        setRefreshingIds((s) => { const n = new Set(s); n.delete(profileId); return n; });
        setLastError(null);
        load();
        return;
      }
      if (r.error) setLastError(r.error);
      if (r.required && r.profileId) {
        setCredentialsModal(r.profileId);
        setCredentialsPrefillUsername(r.prefillUsername ?? '');
      }
      if (r.roles && r.profileId) {
        setCredentialsModal(null);
        const name = dashboardProfiles.find((p) => p.id === r.profileId)?.name ?? 'Profile';
        setRoleModal({ profileId: r.profileId, roles: r.roles, profileName: name });
      }
    } catch (err) {
      setLastError(err instanceof Error ? err.message : String(err));
    } finally {
      setRefreshingIds((s) => { const n = new Set(s); n.delete(profileId); return n; });
    }
  };

  /**
   * Reordering against a filtered list is meaningless — the row above the drop point on screen
   * may not be the row above it in storage — so dragging is disabled while a search is active.
   * Folders holding a match are force-expanded instead, so matches are never hidden behind a
   * collapsed header.
   */
  const searching = searchQuery.trim() !== '';

  const grouped = useMemo(
    () => groupProfilesByFolder(filteredProfiles, folders),
    [filteredProfiles, folders]
  );

  /**
   * Every folder stays on screen while searching, including ones with no match.
   *
   * Filtering to a set of profiles and dragging them into a folder is a primary workflow, and
   * hiding the non-matching folders removes the very target you are aiming at — the folder you
   * want is usually the empty one you just made.
   */
  const isFolderCollapsed = (folderId: string) => {
    if (!collapsedSet.has(folderId)) return false;
    // A collapsed folder holding matches opens itself, so a search never hides a hit.
    const matches = grouped.groups.find((g) => g.folder.id === folderId)?.profiles.length ?? 0;
    return !(searching && matches > 0);
  };

  const profileListClass =
    viewMode === 'list' ? 'space-y-3' : 'grid grid-cols-1 sm:grid-cols-2 gap-4';

  const activeDragProfile = activeDragId?.startsWith('profile:')
    ? dashboardProfiles.find((p) => `profile:${p.id}` === activeDragId) ?? null
    : null;
  const activeDragFolder = activeDragId?.startsWith(FOLDER_SORTABLE_PREFIX)
    ? folders.find((f) => `${FOLDER_SORTABLE_PREFIX}${f.id}` === activeDragId) ?? null
    : null;

  const cardActions: ProfileCardActions = {
    onOpenConsole: handleOpenConsole,
    onRefresh: handleRefresh,
    onEdit: startEdit,
    onDelete: (p) => setDeleteConfirm({ id: p.id, name: p.name }),
    onMoveToFolder: (profileId, folderId) => void handleMoveToFolder(profileId, folderId),
  };

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-3xl font-bold text-discord-text tracking-tight">Profiles</h2>
          <p className="mt-1 text-sm text-discord-textMuted">Manage your AWS profiles and credentials</p>
        </div>
        {/* Split button: adding one profile is the everyday action and stays a single click.
            Importing from Identity Center is a once-at-setup action, so it lives in the menu
            rather than competing for the same visual weight. */}
        <div className="relative" data-add-menu>
          <div className="flex">
            <button
              onClick={startAdd}
              className="inline-flex items-center gap-2 rounded-l-button bg-discord-accent px-5 py-3 text-sm font-semibold text-white shadow-discord-accent hover:bg-discord-accentHover hover:shadow-discord-accent-hover transition-all duration-200"
            >
              <IconPlus className="w-5 h-5" />
              Add profile
            </button>
            <button
              onClick={() => setAddMenuOpen((v) => !v)}
              aria-haspopup="menu"
              aria-expanded={addMenuOpen}
              aria-label="More ways to add profiles"
              className="inline-flex items-center rounded-r-button border-l border-white/20 bg-discord-accent px-2.5 py-3 text-white shadow-discord-accent hover:bg-discord-accentHover transition-all duration-200"
            >
              <IconChevronDown className="w-4 h-4" />
            </button>
          </div>
          {addMenuOpen && (
            <div
              role="menu"
              className="absolute right-0 z-20 mt-2 w-72 overflow-hidden rounded-card border border-discord-border bg-discord-panel shadow-discord-modal animate-modal-in"
            >
              <button
                role="menuitem"
                disabled={ssoBusy}
                onClick={() => {
                  setAddMenuOpen(false);
                  window.dispatchEvent(new Event('wizard:openImport'));
                }}
                className="w-full px-4 py-3 text-left hover:bg-discord-dark disabled:opacity-50 transition-colors"
              >
                <div className="text-sm font-medium text-discord-text">Add accounts…</div>
                <div className="mt-0.5 text-xs text-discord-textMuted">
                  Import SAML or Identity Center accounts in bulk.
                </div>
              </button>
            </div>
          )}
        </div>
      </div>

      {refreshPauseState.paused && (
        <div className="rounded-card border border-amber-500/40 bg-amber-500/10 px-4 py-3 text-amber-100 flex items-start justify-between gap-3">
          <span className="text-sm font-medium">
            {refreshPauseState.pausedDueToFailures
              ? 'Auto-refresh is paused. Resume scheduled refresh in Settings when you have verified your ADFS/IdP credentials.'
              : 'Auto-refresh is paused. Resume scheduled refresh in Settings'}
          </span>
        </div>
      )}

      {networkUnavailable && (
        <div className="rounded-card border border-sky-500/40 bg-sky-500/10 px-4 py-3 text-sky-100 flex items-center justify-between gap-3">
          <span className="text-sm font-medium">
            Network unavailable — will retry automatically when you're back online.
          </span>
          <button
            type="button"
            onClick={() => setNetworkUnavailable(false)}
            className="shrink-0 rounded-button px-2.5 py-1 text-sm hover:bg-sky-500/20 transition-colors"
          >
            Dismiss
          </button>
        </div>
      )}

      {lastError && !networkUnavailable && (
        <div className="rounded-card border border-discord-danger/50 bg-discord-danger/10 px-4 py-3 text-discord-danger flex items-center justify-between gap-3">
          <span className="text-sm font-medium">Error: {lastError}</span>
          <button
            type="button"
            onClick={() => setLastError(null)}
            className="shrink-0 rounded-button px-2.5 py-1 text-sm hover:bg-discord-danger/20 transition-colors"
          >
            Dismiss
          </button>
        </div>
      )}

      <div className="rounded-card bg-discord-panel border border-discord-border overflow-hidden shadow-discord-card">
        {dashboardProfiles.length === 0 && !editing && (
          <div className="py-20 px-8 text-center">
            <div className="mx-auto w-16 h-16 rounded-2xl bg-discord-darkest flex items-center justify-center text-discord-textMuted mb-5">
              <svg className="w-8 h-8" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
              </svg>
            </div>
            <h3 className="text-lg font-semibold text-discord-text">No profiles yet</h3>
            <p className="mt-2 text-sm text-discord-textMuted max-w-sm mx-auto">Add your first AWS SAML profile to get started. You can connect multiple accounts and switch between them.</p>
            <button
              onClick={startAdd}
              className="mt-6 inline-flex items-center gap-2 rounded-button bg-discord-accent px-5 py-3 text-sm font-semibold text-white shadow-discord-accent hover:bg-discord-accentHover hover:shadow-discord-accent-hover transition-all duration-200"
            >
              <IconPlus className="w-5 h-5" />
              Add your first profile
            </button>
          </div>
        )}

        {dashboardProfiles.length > 0 && !editing && (
          <>
            <div className="flex flex-wrap items-center gap-3 px-4 py-3 border-b border-discord-border bg-discord-darkest/40">
              <div className="relative flex-1 min-w-[200px] max-w-md">
                <IconSearch className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-discord-textMuted pointer-events-none" />
                <input
                  type="search"
                  placeholder="Search profiles..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="w-full pl-9 pr-3 py-2 rounded-button border border-discord-border bg-discord-darkest text-discord-text text-sm placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                  aria-label="Search profiles"
                />
              </div>
              <div className="flex items-center rounded-button border border-discord-border bg-discord-darkest p-0.5">
                <Tooltip label="List view" placement="below">
                  <button
                    type="button"
                    onClick={() => setViewMode('list')}
                    className={`p-2 rounded-md transition-colors ${viewMode === 'list' ? 'bg-discord-accent text-white' : 'text-discord-textMuted hover:text-discord-text hover:bg-discord-panel'}`}
                    aria-label="List view"
                    aria-pressed={viewMode === 'list'}
                  >
                    <IconList className="w-4 h-4" />
                  </button>
                </Tooltip>
                <Tooltip label="Grid view" placement="below">
                  <button
                    type="button"
                    onClick={() => setViewMode('grid')}
                    className={`p-2 rounded-md transition-colors ${viewMode === 'grid' ? 'bg-discord-accent text-white' : 'text-discord-textMuted hover:text-discord-text hover:bg-discord-panel'}`}
                    aria-label="Grid view"
                    aria-pressed={viewMode === 'grid'}
                  >
                    <IconGrid className="w-4 h-4" />
                  </button>
                </Tooltip>
              </div>
              <Tooltip label="New folder" placement="below" align="right">
                <button
                  type="button"
                  onClick={() => void handleCreateFolder()}
                  className="rounded-button border border-discord-border bg-discord-darkest p-2 text-discord-text hover:border-discord-borderLight hover:bg-discord-panel transition-colors"
                  aria-label="New folder"
                >
                  <IconFolderPlus className="w-4 h-4" />
                </button>
              </Tooltip>
            </div>

            {filteredProfiles.length === 0 ? (
              <div className="py-12 text-center">
                <p className="text-discord-textMuted text-sm">No profiles match &quot;{searchQuery}&quot;</p>
                <button
                  type="button"
                  onClick={() => setSearchQuery('')}
                  className="mt-2 text-sm text-discord-accent hover:underline"
                >
                  Clear search
                </button>
              </div>
            ) : (
              <DndContext
                sensors={sensors}
                collisionDetection={closestCenter}
                onDragStart={handleDragStart}
                onDragEnd={handleDragEnd}
                onDragCancel={handleDragCancel}
              >
                <div className="p-4 space-y-3">
                  <SortableContext
                    items={folders.map((f) => `${FOLDER_SORTABLE_PREFIX}${f.id}`)}
                    strategy={verticalListSortingStrategy}
                  >
                    {grouped.groups.map((g) => (
                      <ProfileFolderSection
                        key={g.folder.id}
                        folder={g.folder}
                        count={g.profiles.length}
                        collapsed={isFolderCollapsed(g.folder.id)}
                        emptyLabel={
                          searching
                            ? 'No matches — drop here to move into this folder'
                            : 'Empty — drag profiles here'
                        }
                        onToggleCollapsed={() => toggleFolderCollapsed(g.folder.id)}
                        onRename={(name) => void handleRenameFolder(g.folder.id, name)}
                        onRecolor={(color) => void handleRecolorFolder(g.folder.id, color)}
                        onDelete={() =>
                          setFolderDeleteConfirm({
                            id: g.folder.id,
                            name: g.folder.name,
                            count: g.profiles.length,
                          })
                        }
                      >
                        <SortableContext
                          items={g.profiles.map((p) => `profile:${p.id}`)}
                          strategy={verticalListSortingStrategy}
                        >
                          <div className={profileListClass}>
                            {g.profiles.map((p) => (
                              <SortableProfileCard
                                key={p.id}
                                p={p}
                                viewMode={viewMode}
                                folders={folders}
                                refreshing={refreshingIds.has(p.id)}
                                refreshDisabled={refreshingIds.size > 0}
                                openingConsole={openingConsoleIds.has(p.id)}
                                actions={cardActions}
                                onContextMenu={handleProfileContextMenu}
                              />
                            ))}
                          </div>
                        </SortableContext>
                      </ProfileFolderSection>
                    ))}
                  </SortableContext>

                  {/* Ungrouped. The heading only appears once folders exist — with none, the page
                      looks exactly as it did before folders were introduced. */}
                  <UngroupedDropZone showHeading={folders.length > 0}>
                    <SortableContext
                      items={grouped.ungrouped.map((p) => `profile:${p.id}`)}
                      strategy={verticalListSortingStrategy}
                    >
                      <div className={profileListClass}>
                        {grouped.ungrouped.map((p) => (
                          <SortableProfileCard
                            key={p.id}
                            p={p}
                            viewMode={viewMode}
                            folders={folders}
                            refreshing={refreshingIds.has(p.id)}
                            refreshDisabled={refreshingIds.size > 0}
                            openingConsole={openingConsoleIds.has(p.id)}
                            actions={cardActions}
                            onContextMenu={handleProfileContextMenu}
                          />
                        ))}
                      </div>
                    </SortableContext>
                  </UngroupedDropZone>
                </div>

                {/* Drags the whole card, not just the grip icon the pointer is on. */}
                <DragOverlay>
                  {activeDragProfile ? (
                    <div
                      className={`${cardShellClass(viewMode)} border-discord-accent bg-discord-panel shadow-discord-modal`}
                    >
                      <ProfileCardBody
                        p={activeDragProfile}
                        viewMode={viewMode}
                        folders={folders}
                        handle={
                          <span className={`flex-shrink-0 ${viewMode === 'list' ? 'p-2' : 'p-1.5'} text-discord-textMuted`}>
                            <IconGrip className="w-4 h-4" />
                          </span>
                        }
                        refreshing={false}
                        refreshDisabled
                        openingConsole={false}
                        actions={cardActions}
                      />
                    </div>
                  ) : activeDragFolder ? (
                    <div className="rounded-card border border-discord-accent bg-discord-panel px-4 py-3 text-sm font-semibold text-discord-text shadow-discord-modal">
                      {activeDragFolder.name}
                    </div>
                  ) : null}
                </DragOverlay>
              </DndContext>
            )}
          </>
        )}

        {editing && (
          <div className="border-t border-discord-border bg-discord-darkest/30 p-8">
            <h3 className="text-xl font-bold text-discord-text mb-6">
              {dashboardProfiles.some((p) => p.id === editing.id) ? 'Edit profile' : 'New profile'}
            </h3>
            <div className="grid gap-4 sm:grid-cols-2">
              <div>
                <label className="block text-sm font-medium text-discord-textMuted">Profile name <span className="text-discord-danger">*</span></label>
                <input
                  value={form.name}
                  onChange={(e) => {
                    const name = e.target.value;
                    const isExistingProfile = editing && dashboardProfiles.some((p) => p.id === editing.id);
                    if (isExistingProfile) {
                      setForm((f) => ({ ...f, name }));
                    } else {
                      setForm((f) => ({
                        ...f,
                        name,
                        credentialProfileName:
                          f.credentialProfileName === f.name || f.credentialProfileName === '' ? name : f.credentialProfileName,
                      }));
                    }
                  }}
                  className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                  placeholder="e.g. saml"
                />
              </div>
              <div>
                <label className="flex items-center gap-1.5 text-sm font-medium text-discord-textMuted">
                  Credentials section name <span className="text-discord-danger">*</span>
                  <Tooltip
                    label="Section name in your AWS credentials file. Use this with the AWS CLI --profile option and other tools to use these credentials."
                    placement="below"
                    wrap
                    wrapWidth="2xl"
                  >
                    <span
                      className="inline-flex items-center justify-center w-5 h-5 rounded-full text-discord-textMuted hover:text-discord-text transition-colors cursor-help"
                      aria-label="Help"
                    >
                      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={1.5} aria-hidden>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M9.879 7.519c1.171-1.025 3.071-1.025 4.242 0 1.172 1.025 1.172 2.687 0 3.712-.203.179-.43.326-.67.442-.745.361-1.45.999-1.45 1.827v.75M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9 5.25h.008v.008H12v-.008z" />
                      </svg>
                    </span>
                  </Tooltip>
                </label>
                <input
                  value={form.credentialProfileName}
                  onChange={(e) => setForm((f) => ({ ...f, credentialProfileName: e.target.value }))}
                  className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                  placeholder="Same as profile name"
                />
              </div>
              <div className="sm:col-span-2">
                <label className="block text-sm font-medium text-discord-textMuted mb-1.5">Sign-in method</label>
                <div className="flex gap-2">
                  {(
                    [
                      { key: 'saml', title: 'SAML / ADFS', blurb: 'Stored username + password, refreshed silently' },
                      { key: 'identityCenter', title: 'Identity Center', blurb: 'Browser sign-in, then silent renewal' },
                    ] as { key: ProfileAuthType; title: string; blurb: string }[]
                  ).map((opt) => (
                    <button
                      key={opt.key}
                      type="button"
                      onClick={() => {
                        setForm((f) => ({
                          ...f,
                          authType: opt.key,
                          // Fill the org from Settings on switching, without clobbering anything
                          // the user (or an existing profile) already has.
                          ...(opt.key === 'identityCenter'
                            ? {
                                ssoStartUrl: f.ssoStartUrl?.trim() || ssoDefaults.startUrl,
                                ssoRegion: f.ssoRegion?.trim() || ssoDefaults.region,
                              }
                            : {}),
                        }));
                        setSsoAccounts(null);
                        setSsoIdentity(null);
                      }}
                      className={`flex-1 rounded-button border px-3 py-2 text-left transition-colors ${
                        formAuthType === opt.key
                          ? 'border-discord-accent bg-discord-accent/10 text-discord-text'
                          : 'border-discord-border bg-discord-darkest text-discord-textMuted hover:text-discord-text'
                      }`}
                    >
                      <div className="text-sm font-medium">{opt.title}</div>
                      <div className="text-xs text-discord-textMuted">{opt.blurb}</div>
                    </button>
                  ))}
                </div>
              </div>

              {!isSsoForm && (
                <>
                  <div className="sm:col-span-2">
                    <label className="block text-sm font-medium text-discord-textMuted">IdP entry URL <span className="text-discord-danger">*</span></label>
                    <input
                      value={form.idpEntryUrl}
                      onChange={(e) => setForm((f) => ({ ...f, idpEntryUrl: e.target.value }))}
                      className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                      placeholder="https://adfs.example.com/adfs/ls/..."
                    />
                  </div>
                  <div className="sm:col-span-2">
                    <label className="block text-sm font-medium text-discord-textMuted">Role / Account <span className="text-discord-danger">*</span></label>
                    <p className="mt-0.5 text-xs text-discord-textMuted mb-1">
                      Choose the AWS role (account) for this profile. Load the list by signing in; use default credentials or you will be prompted.
                    </p>
                    <div className="flex gap-2">
                      <select
                        value={form.roleArn ?? ''}
                        onChange={(e) => {
                          const roleArn = e.target.value;
                          const role = rolesForIdp?.find((r) => r.roleArn === roleArn);
                          if (role) {
                            setForm((f) => ({
                              ...f,
                              roleArn: role.roleArn,
                              principalArn: role.principalArn,
                              roleDisplayText: roleToDisplayText(role, accountDisplayNames),
                            }));
                          } else {
                            setForm((f) => ({ ...f, roleArn: undefined, principalArn: undefined, roleDisplayText: undefined }));
                          }
                        }}
                        className="flex-1 rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text focus:border-discord-accent focus:outline-none transition-colors"
                      >
                        <option value="">Select a role…</option>
                        {rolesForIdp?.map((r) => (
                          <option key={r.roleArn} value={r.roleArn}>
                            {roleToDisplayText(r, accountDisplayNames)}
                          </option>
                        ))}
                      </select>
                      <Tooltip label="Load or refresh role list" placement="left">
                        <button
                          type="button"
                          onClick={handleRefreshRoles}
                          disabled={loadingRoles || !form.idpEntryUrl?.trim()}
                          className="inline-flex items-center justify-center rounded-button border border-discord-border bg-discord-darkest p-2 text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors disabled:opacity-50"
                        >
                          <IconRefresh className={`w-5 h-5 ${loadingRoles ? 'animate-spin' : ''}`} />
                        </button>
                      </Tooltip>
                    </div>
                  </div>
                </>
              )}

              {isSsoForm && (
                <>
                  <div className="sm:col-span-2">
                    <label className="block text-sm font-medium text-discord-textMuted">SSO start URL <span className="text-discord-danger">*</span></label>
                    <p className="mt-0.5 text-xs text-discord-textMuted mb-1">
                      From the AWS access portal: open any account → <em>Access keys</em> → the
                      “AWS IAM Identity Center credentials” tab. Pasting the URL with a trailing
                      <code className="mx-1 rounded bg-discord-darkest px-1">#/</code> is fine.
                    </p>
                    <input
                      value={form.ssoStartUrl ?? ''}
                      onChange={(e) => setForm((f) => ({ ...f, ssoStartUrl: e.target.value }))}
                      className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                      placeholder="https://d-xxxxxxxxxx.awsapps.com/start"
                    />
                  </div>
                  <div className="sm:col-span-2">
                    <label className="block text-sm font-medium text-discord-textMuted">SSO region <span className="text-discord-danger">*</span></label>
                    <div className="mt-1.5 flex gap-2">
                      <input
                        value={form.ssoRegion ?? ''}
                        onChange={(e) => setForm((f) => ({ ...f, ssoRegion: e.target.value }))}
                        className="flex-1 rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                        placeholder="us-west-2"
                      />
                      <Tooltip label="Probe for the region (the portal shows it too)" placement="left">
                        <button
                          type="button"
                          onClick={handleDetectSsoRegion}
                          disabled={ssoBusy || !form.ssoStartUrl?.trim()}
                          className="rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors disabled:opacity-50"
                        >
                          Detect
                        </button>
                      </Tooltip>
                    </div>
                  </div>
                  <div className="sm:col-span-2">
                    <label className="block text-sm font-medium text-discord-textMuted">Account / Role <span className="text-discord-danger">*</span></label>
                    <p className="mt-0.5 text-xs text-discord-textMuted mb-1">
                      Sign in once to load the accounts and permission sets assigned to you.
                    </p>
                    <div className="flex gap-2">
                      <select
                        value={form.ssoAccountId && form.ssoRoleName ? `${form.ssoAccountId}|${form.ssoRoleName}` : ''}
                        onChange={(e) => {
                          const [accountId, roleName] = e.target.value.split('|');
                          const account = ssoAccounts?.find((a) => a.accountId === accountId);
                          if (account && roleName) {
                            setForm((f) => ({
                              ...f,
                              ssoAccountId: accountId,
                              ssoRoleName: roleName,
                              roleDisplayText: ssoRoleDisplayText(account, roleName, accountDisplayNames),
                              name: f.name?.trim() ? f.name : `${account.accountName} ${roleName}`,
                            }));
                          } else {
                            setForm((f) => ({ ...f, ssoAccountId: undefined, ssoRoleName: undefined, roleDisplayText: undefined }));
                          }
                        }}
                        className="flex-1 rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text focus:border-discord-accent focus:outline-none transition-colors"
                      >
                        <option value="">
                          {ssoAccounts ? 'Select an account and role…' : 'Sign in to load accounts…'}
                        </option>
                        {ssoAccounts?.flatMap((a) =>
                          a.roles.map((roleName) => (
                            <option key={`${a.accountId}|${roleName}`} value={`${a.accountId}|${roleName}`}>
                              {ssoRoleDisplayText(a, roleName, accountDisplayNames)}
                            </option>
                          ))
                        )}
                      </select>
                      <Tooltip label="Sign in and load accounts" placement="left">
                        <button
                          type="button"
                          onClick={handleLoadSsoAccounts}
                          disabled={ssoBusy || !form.ssoStartUrl?.trim() || !form.ssoRegion?.trim()}
                          className="inline-flex items-center justify-center rounded-button border border-discord-border bg-discord-darkest p-2 text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors disabled:opacity-50"
                        >
                          <IconRefresh className={`w-5 h-5 ${ssoBusy ? 'animate-spin' : ''}`} />
                        </button>
                      </Tooltip>
                    </div>
                    {ssoIdentity && (
                      // Surfaced deliberately: in orgs with separate day-to-day and privileged
                      // accounts, signing in as the wrong identity is easy and otherwise silent.
                      <p className="mt-1.5 text-xs text-discord-textMuted">
                        Signed in as <span className="text-discord-text">{ssoIdentity}</span>
                      </p>
                    )}
                  </div>
                </>
              )}
              <div>
                <label className="block text-sm font-medium text-discord-textMuted">Description</label>
                <input
                  value={form.label}
                  onChange={(e) => setForm((f) => ({ ...f, label: e.target.value }))}
                  className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                  placeholder="e.g. Production"
                />
              </div>
              <div className="sm:col-span-2 rounded-card border border-discord-border bg-discord-panel/50 p-4">
                <label className="block text-sm font-medium text-discord-textMuted mb-3">Profile icon</label>
                <ProfileIconPicker
                  iconName={form.iconName}
                  iconColor={form.iconColor}
                  onChange={(iconName, iconColor) => setForm((f) => ({ ...f, iconName, iconColor }))}
                />
              </div>
              <div className="flex items-center gap-4 sm:col-span-2">
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={form.autoRefresh}
                    onChange={(e) => setForm((f) => ({ ...f, autoRefresh: e.target.checked }))}
                    className="rounded border-discord-border text-discord-accent focus:ring-discord-accent"
                  />
                  <span className="text-sm text-discord-textMuted">Auto refresh</span>
                </label>
                <div className="flex items-center gap-2">
                  <input
                    type="number"
                    min={1}
                    max={12}
                    value={Math.max(1, Math.floor(form.refreshIntervalMinutes / 60))}
                    onChange={(e) => {
                      const hours = Math.max(1, Math.min(12, parseInt(e.target.value, 10) || 1));
                      setForm((f) => ({ ...f, refreshIntervalMinutes: hours * 60 }));
                    }}
                    className="w-16 rounded-button border border-discord-border bg-discord-darkest px-2 py-1.5 text-discord-text focus:border-discord-accent focus:outline-none"
                  />
                  {/* For Identity Center this value only drives the refresh schedule. The portal's
                      GetRoleCredentials takes no duration parameter — the session length comes
                      from the permission set — so calling it "session length" here would be wrong. */}
                  {isSsoForm ? (
                    <Tooltip
                      label="Sets how often this profile refreshes. Session length is fixed by your Identity Center admin and can't be changed here."
                      placement="above"
                      align="left"
                      wrap
                      wrapWidth="md"
                      usePortal
                    >
                      <span className="flex items-center gap-1 text-sm text-discord-textMuted">
                        hours (refresh interval)
                        <svg className="h-3.5 w-3.5 opacity-70" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                        </svg>
                      </span>
                    </Tooltip>
                  ) : (
                    <span className="text-sm text-discord-textMuted">hours (refresh interval and session length)</span>
                  )}
                </div>
              </div>
              {/* Stored IdP credentials are meaningless for Identity Center — there is no
                  supported way to replay a password into the browser sign-in. */}
              {!isSsoForm && (
                <div className="sm:col-span-2">
                  <label className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={form.useDefaultCredentials ?? false}
                      onChange={(e) => setForm((f) => ({ ...f, useDefaultCredentials: e.target.checked }))}
                      className="rounded border-discord-border text-discord-accent focus:ring-discord-accent"
                    />
                    <span className="text-sm text-discord-textMuted">Use default credentials</span>
                  </label>
                  <p className="mt-1 text-xs text-discord-textMuted">
                    When on, refresh uses the username/password from Settings → Default credentials. When off, you are
                    prompted for username and password every time you refresh.
                  </p>
                </div>
              )}
              {isSsoForm && (
                <div className="sm:col-span-2 rounded-card border border-discord-border bg-discord-panel/50 p-4">
                  <p className="text-xs text-discord-textMuted">
                    Identity Center profiles renew silently in the background. A browser sign-in is
                    only needed when the SSO session itself expires — your saved IdP username and
                    password are not used and cannot be.
                  </p>
                </div>
              )}
            </div>
            <div className="mt-8 flex gap-3">
              <button
                onClick={save}
                disabled={!requiredFieldsValid}
                className="inline-flex items-center gap-2 rounded-button bg-discord-accent px-5 py-3 text-sm font-semibold text-white shadow-discord-accent hover:bg-discord-accentHover hover:shadow-discord-accent-hover disabled:opacity-50 disabled:cursor-not-allowed disabled:shadow-none transition-all duration-200"
              >
                Save
              </button>
              <button
                onClick={() => {
                  setEditing(null);
                  setFetchRolesModal(null);
                }}
                className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2.5 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>

      {roleModal && (
        <RoleModal
          profileId={roleModal.profileId}
          profileName={roleModal.profileName}
          roles={roleModal.roles}
          onClose={() => { setRoleModal(null); setRefreshingIds((s) => { const n = new Set(s); n.delete(roleModal.profileId); return n; }); }}
          onSelect={async (index) => {
            await window.electron.selectRole(roleModal.profileId, index);
            setRoleModal(null);
            setRefreshingIds((s) => { const n = new Set(s); n.delete(roleModal.profileId); return n; });
            load();
          }}
        />
      )}
      {credentialsModal && (
        <CredentialsModal
          profileId={credentialsModal}
          profileName={dashboardProfiles.find((p) => p.id === credentialsModal)?.name ?? 'Profile'}
          initialUsername={credentialsPrefillUsername}
          onClose={() => {
            setCredentialsModal(null);
            setCredentialsPrefillUsername('');
            setRefreshingIds(new Set());
          }}
          onSubmit={handleSubmitCredentials}
        />
      )}
      {refreshAllModal && (
        <CredentialsModal
          profileId=""
          profileName=""
          title="Sign in to refresh all profiles"
          description="These credentials will be used for all profiles that don't use default credentials. Profiles using default credentials will be refreshed separately."
          initialUsername=""
          onClose={() => setRefreshAllModal(null)}
          onSubmit={async (_profileId, username, password) => {
            const { credentialProfileIds, defaultProfileIds } = refreshAllModal;
            await (window.electron as { submitCredentialsForRefreshAll?: (a: string[], b: string[], u: string, p: string) => Promise<void> }).submitCredentialsForRefreshAll?.(
              credentialProfileIds,
              defaultProfileIds,
              username,
              password
            );
            setRefreshAllModal(null);
            setLastError(null);
            load();
          }}
        />
      )}
      {fetchRolesModal && (
        <CredentialsModal
          profileId=""
          profileName=""
          title="Sign in to load roles"
          description="Enter your IdP username and password to fetch the list of roles for this IdP."
          initialUsername={fetchRolesModal.prefillUsername ?? ''}
          onClose={() => setFetchRolesModal(null)}
          onSubmit={async (_profileId, username, password) => {
            await handleFetchRolesSubmit(username, password);
          }}
        />
      )}
      {ssoLoginNotice && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center modal-backdrop p-4"
          onClick={() => setSsoLoginNotice(null)}
        >
          <div
            className="w-full max-w-md rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal animate-modal-in"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold text-discord-text">Sign in to AWS</h3>
            <p className="mt-2 text-sm text-discord-textMuted">
              Your Identity Center session has expired. Signing in once renews every profile in
              this organization.
            </p>
            <p className="mt-2 break-all text-xs text-discord-textMuted">{ssoLoginNotice.startUrl}</p>
            <div className="mt-6 flex gap-3">
              <button
                disabled={ssoBusy}
                onClick={async () => {
                  setSsoBusy(true);
                  try {
                    const result = await window.electron.ssoSignIn(
                      ssoLoginNotice.startUrl,
                      ssoLoginNotice.region
                    );
                    if (result.success) {
                      setSsoLoginNotice(null);
                      // One sign-in unblocks every profile in the org.
                      await window.electron.refreshAutoRefreshProfiles();
                      load();
                    } else {
                      setLastError(result.error);
                    }
                  } finally {
                    setSsoBusy(false);
                  }
                }}
                className="inline-flex items-center gap-2 rounded-button bg-discord-accent px-5 py-2.5 text-sm font-semibold text-white hover:bg-discord-accentHover disabled:opacity-50 transition-all"
              >
                {ssoBusy ? 'Waiting for browser…' : 'Sign in'}
              </button>
              <button
                onClick={() => setSsoLoginNotice(null)}
                className="rounded-button border border-discord-border px-5 py-2.5 text-sm text-discord-textMuted hover:text-discord-text transition-colors"
              >
                Later
              </button>
            </div>
          </div>
        </div>
      )}
      {autoRefreshFailureModalOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center modal-backdrop p-4"
          onClick={() => setAutoRefreshFailureModalOpen(false)}
        >
          <div
            className="w-full max-w-md rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal animate-modal-in"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold text-discord-text">Auto-refresh paused</h3>
            <p className="mt-2 text-sm text-discord-textMuted">
              You received multiple consecutive authentication failures, so automatic credential refresh was paused. Check your credentials (including a recent password change) before resuming auto-refresh in Settings.
            </p>
            <div className="mt-6 flex justify-end">
              <button
                type="button"
                onClick={() => setAutoRefreshFailureModalOpen(false)}
                className="rounded-button bg-discord-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90 transition-opacity"
              >
                OK
              </button>
            </div>
          </div>
        </div>
      )}
      {deleteConfirm && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center modal-backdrop p-4"
          onClick={() => setDeleteConfirm(null)}
        >
          <div
            className="w-full max-w-md rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal animate-modal-in"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold text-discord-text">Delete profile</h3>
            <p className="mt-2 text-sm text-discord-textMuted">
              Delete &quot;{deleteConfirm.name}&quot;? This will also remove its credentials from the credentials file.
            </p>
            <div className="mt-6 flex justify-end gap-2">
              <button
                onClick={() => setDeleteConfirm(null)}
                className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={() => deleteConfirm && remove(deleteConfirm.id)}
                className="rounded-button bg-discord-danger px-4 py-2 text-sm font-medium text-white hover:opacity-90 transition-opacity"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}

      {contextMenu &&
        (() => {
          // Resolved fresh rather than captured: the 10s poll may have replaced the summary, and
          // a profile deleted while the menu is open should close it rather than act on a ghost.
          const target = dashboardProfiles.find((p) => p.id === contextMenu.profileId);
          return target ? (
            <ProfileContextMenu
              x={contextMenu.x}
              y={contextMenu.y}
              profile={target}
              folders={folders}
              onMove={(profileId, folderId) => void handleMoveToFolder(profileId, folderId)}
              onClose={closeContextMenu}
            />
          ) : null;
        })()}

      {folderDeleteConfirm && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center modal-backdrop p-4"
          onClick={() => setFolderDeleteConfirm(null)}
        >
          <div
            className="w-full max-w-md rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal animate-modal-in"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold text-discord-text">Delete folder</h3>
            <p className="mt-2 text-sm text-discord-textMuted">
              Delete the folder &quot;{folderDeleteConfirm.name}&quot;?
              {folderDeleteConfirm.count > 0
                ? ` The ${folderDeleteConfirm.count} profile${
                    folderDeleteConfirm.count === 1 ? '' : 's'
                  } inside will move to Ungrouped — no profile or credential is deleted.`
                : ' It is empty.'}
            </p>
            <div className="mt-6 flex justify-end gap-2">
              <button
                onClick={() => setFolderDeleteConfirm(null)}
                className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={() => void handleDeleteFolder(folderDeleteConfirm.id)}
                className="rounded-button bg-discord-danger px-4 py-2 text-sm font-medium text-white hover:opacity-90 transition-opacity"
              >
                Delete folder
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function RoleModal({
  profileId,
  profileName,
  roles,
  onClose,
  onSelect,
}: {
  profileId: string;
  profileName: string;
  roles: AwsRole[];
  onClose: () => void;
  onSelect: (index: number) => void;
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center modal-backdrop p-4" onClick={onClose}>
      <div className="w-full max-w-lg rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal animate-modal-in" onClick={(e) => e.stopPropagation()}>
        <h3 className="text-lg font-semibold text-discord-text">Choose role for {profileName}</h3>
        <p className="mt-1 text-sm text-discord-textMuted">Select which AWS role to assume.</p>
        <ul className="mt-4 space-y-2">
          {roles.map((r, i) => (
            <li key={i}>
              <button
                onClick={() => onSelect(i)}
                className="w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2.5 text-left text-sm text-discord-text hover:bg-discord-dark transition-colors"
              >
                {r.displayText}
              </button>
            </li>
          ))}
        </ul>
        <div className="mt-4">
          <button onClick={onClose} className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors">
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

function CredentialsModal({
  profileId,
  profileName,
  initialUsername = '',
  title,
  description,
  onClose,
  onSubmit,
}: {
  profileId: string;
  profileName: string;
  initialUsername?: string;
  title?: string;
  description?: string;
  onClose: () => void;
  onSubmit: (profileId: string, username: string, password: string) => void | Promise<void>;
}) {
  const [username, setUsername] = useState(initialUsername);
  const [password, setPassword] = useState('');
  const [submitting, setSubmitting] = useState(false);
  useEffect(() => {
    setUsername(initialUsername);
  }, [initialUsername]);

  const handleSubmit = async () => {
    if (!username || !password || submitting) return;
    setSubmitting(true);
    try {
      await Promise.resolve(onSubmit(profileId, username, password));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center modal-backdrop p-4" onClick={onClose}>
      <div className="w-full max-w-md rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal animate-modal-in" onClick={(e) => e.stopPropagation()}>
        <h3 className="text-lg font-semibold text-discord-text">{title ?? `Sign in for ${profileName}`}</h3>
        <p className="mt-1 text-sm text-discord-textMuted">
          {description ?? 'Enter your IdP username and password. To skip this prompt next time, set default credentials in Settings.'}
        </p>
        <div className="mt-4 space-y-3">
          <input
            type="text"
            placeholder="Username"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            className="w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
          />
          <input
            type="password"
            placeholder="Password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
          />
        </div>
        <div className="mt-6 flex justify-end gap-2">
          <button onClick={onClose} disabled={submitting} className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors disabled:opacity-50">
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={!username || !password || submitting}
            className="inline-flex items-center gap-2 rounded-button bg-discord-accent px-4 py-2 text-sm font-medium text-white hover:bg-discord-accentHover transition-colors disabled:opacity-50"
          >
            Sign in
            {submitting && (
              <svg className="h-4 w-4 animate-spin" fill="none" viewBox="0 0 24 24" aria-hidden>
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
              </svg>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
