import { useEffect, useState } from 'react';
import type { Profile, DashboardProfileSummary, AwsRole } from '../../shared/types';
import { v4 as uuidv4 } from 'uuid';

declare global {
  interface Window {
    electron: {
      getProfiles: () => Promise<Profile[]>;
      getProfileById: (id: string) => Promise<Profile | null>;
      getDashboardState: () => Promise<DashboardProfileSummary[]>;
      reorderProfiles: (orderedIds: string[]) => Promise<void>;
      getSettings: () => Promise<{ defaultIdpEntryUrl?: string; accountDisplayNames?: Record<string, string> }>;
      saveProfile: (profile: Profile) => Promise<void>;
      deleteProfile: (id: string) => Promise<void>;
      refreshProfile: (profileId: string) => Promise<unknown>;
      submitCredentials: (profileId: string, username: string, password: string) => Promise<unknown>;
      selectRole: (profileId: string, roleIndex: number) => Promise<unknown>;
      onCredentialsRequired: (cb: (profileId: string, prefillUsername?: string) => void) => void;
      onCredentialsRefreshed: (cb: (profileId: string) => void) => void;
      onRefreshStarted: (cb: (profileId: string) => void) => void;
      onCredentialsExpired: (cb: (profileId: string, message: string) => void) => void;
      getCachedRoles: (idpEntryUrl: string) => Promise<AwsRole[] | null>;
      fetchRoles: (idpEntryUrl: string, useDefaultCredentials: boolean, profileId?: string) => Promise<unknown>;
      fetchRolesWithCredentials: (idpEntryUrl: string, username: string, password: string) => Promise<unknown>;
      getDefaultCredentialsDisplay: () => Promise<{ username: string; hasPassword: boolean } | null>;
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
const IconGrip = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path d="M8 6a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0zm6-12a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0z" />
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
  refreshIntervalHours: 1,
  useDefaultCredentials: false,
  credentialProfileName: '',
});

export default function Profiles() {
  const [dashboardProfiles, setDashboardProfiles] = useState<DashboardProfileSummary[]>([]);
  const [editing, setEditing] = useState<Profile | null>(null);
  const [form, setForm] = useState<Profile>(emptyProfile());
  const [refreshingIds, setRefreshingIds] = useState<Set<string>>(new Set());
  const [credentialsModal, setCredentialsModal] = useState<string | null>(null);
  const [credentialsPrefillUsername, setCredentialsPrefillUsername] = useState('');
  const [roleModal, setRoleModal] = useState<{ profileId: string; roles: AwsRole[]; profileName: string } | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const [rolesForIdp, setRolesForIdp] = useState<AwsRole[] | null>(null);
  const [loadingRoles, setLoadingRoles] = useState(false);
  const [fetchRolesModal, setFetchRolesModal] = useState<{ idpEntryUrl: string; prefillUsername?: string } | null>(null);
  const [accountDisplayNames, setAccountDisplayNames] = useState<Record<string, string>>({});
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<{ id: string; name: string } | null>(null);

  const load = () => window.electron.getDashboardState().then(setDashboardProfiles);

  useEffect(() => {
    load();
    const interval = setInterval(load, 10000);
    return () => clearInterval(interval);
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
      setCredentialsModal(null);
      setLastError(null);
      load();
    });
    window.electron.onRefreshStarted((profileId) => {
      setRefreshingIds((s) => new Set(s).add(profileId));
    });
    window.electron.onCredentialsExpired((profileId, message) => {
      setRefreshingIds((s) => { const n = new Set(s); n.delete(profileId); return n; });
      setLastError(message);
      load();
    });
  }, []);

  const startAdd = async () => {
    const newProfile = emptyProfile();
    const [settings, defaultCreds] = await Promise.all([
      window.electron.getSettings(),
      window.electron.getDefaultCredentialsDisplay(),
    ]);
    setAccountDisplayNames(settings?.accountDisplayNames ?? {});
    const idpUrl = settings?.defaultIdpEntryUrl ?? '';
    const useDefault = !!defaultCreds;
    setForm({ ...newProfile, idpEntryUrl: idpUrl, useDefaultCredentials: useDefault });
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

  const requiredFieldsValid =
    !!form.name?.trim() &&
    !!form.credentialProfileName?.trim() &&
    !!form.idpEntryUrl?.trim() &&
    !!form.roleArn?.trim() &&
    !!form.label?.trim();

  const save = async () => {
    if (!requiredFieldsValid) {
      setLastError('Please fill in all required fields: Profile name, Credentials section name, IdP entry URL, Role / Account, and Friendly label.');
      return;
    }
    setLastError(null);
    const toSave = { ...form, credentialProfileName: form.credentialProfileName || form.name };
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
      };
      if (r.required && r.profileId) {
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

  const handleDragStart = (e: React.DragEvent, profileId: string) => {
    setDraggedId(profileId);
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', profileId);
  };

  const handleDragOver = (e: React.DragEvent, profileId: string) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    if (draggedId && draggedId !== profileId) setDropTargetId(profileId);
  };

  const handleDragLeave = () => {
    setDropTargetId(null);
  };

  const handleDrop = (e: React.DragEvent, dropTargetId: string) => {
    e.preventDefault();
    setDropTargetId(null);
    setDraggedId(null);
    const sourceId = e.dataTransfer.getData('text/plain');
    if (!sourceId || sourceId === dropTargetId) return;
    const ids = dashboardProfiles.map((p) => p.id);
    const fromIdx = ids.indexOf(sourceId);
    const toIdx = ids.indexOf(dropTargetId);
    if (fromIdx === -1 || toIdx === -1) return;
    const newOrder = [...ids];
    newOrder.splice(fromIdx, 1);
    newOrder.splice(toIdx, 0, sourceId);
    window.electron.reorderProfiles(newOrder).then(load);
  };

  const handleDragEnd = () => {
    setDraggedId(null);
    setDropTargetId(null);
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

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h2 className="text-2xl font-semibold text-discord-text">Profiles</h2>
        <button
          onClick={startAdd}
          className="inline-flex items-center gap-2 rounded-lg bg-discord-accent px-4 py-2.5 text-sm font-medium text-white shadow-sm transition hover:bg-discord-accentHover"
        >
          <IconPlus className="w-4 h-4" />
          Add profile
        </button>
      </div>

      {lastError && (
        <div className="rounded-lg border border-discord-danger/50 bg-discord-danger/10 px-4 py-3 text-discord-danger flex items-center justify-between gap-3">
          <span className="text-sm"><strong>Error:</strong> {lastError}</span>
          <button
            type="button"
            onClick={() => setLastError(null)}
            className="shrink-0 rounded px-2 py-1 text-sm hover:bg-discord-danger/20"
          >
            Dismiss
          </button>
        </div>
      )}

      <div className="rounded-xl bg-discord-panel shadow ring-1 ring-discord-darkest/50 overflow-hidden">
        {dashboardProfiles.length === 0 && !editing && (
          <div className="py-12 text-center">
            <p className="text-discord-textMuted">No profiles yet.</p>
            <button
              onClick={startAdd}
              className="mt-3 inline-flex items-center gap-2 rounded-lg bg-discord-accent px-4 py-2 text-sm text-white hover:bg-discord-accentHover"
            >
              <IconPlus className="w-4 h-4" />
              Add your first profile
            </button>
          </div>
        )}

        {dashboardProfiles.length > 0 && !editing && (
          <div className="overflow-x-auto">
            <table className="w-full text-left text-sm min-w-[720px]">
              <thead>
                <tr className="border-b border-discord-darkest bg-discord-darkest/30 text-discord-textMuted">
                  <th className="w-9 px-1 py-3" aria-label="Drag to reorder" />
                  <th className="px-4 py-3 font-medium whitespace-nowrap">Profile</th>
                  <th className="px-4 py-3 font-medium whitespace-nowrap">Account / Label</th>
                  <th className="px-4 py-3 font-medium whitespace-nowrap">Status</th>
                  <th className="px-4 py-3 font-medium whitespace-nowrap">Time left</th>
                  <th className="px-4 py-3 font-medium whitespace-nowrap">Expires (PST)</th>
                  <th className="px-4 py-3 font-medium whitespace-nowrap w-0 text-right">Actions</th>
                </tr>
              </thead>
              <tbody>
                {dashboardProfiles.map((p) => (
                  <tr
                    key={p.id}
                    className={`border-b border-discord-darkest/50 transition ${
                      draggedId === p.id ? 'opacity-50' : ''
                    } ${dropTargetId === p.id ? 'bg-discord-accent/20 ring-1 ring-inset ring-discord-accent' : 'hover:bg-discord-darkest/20'}`}
                    onDragOver={(e) => handleDragOver(e, p.id)}
                    onDragLeave={handleDragLeave}
                    onDrop={(e) => handleDrop(e, p.id)}
                  >
                    <td
                      className="w-9 cursor-grab px-1 py-3 text-discord-textMuted hover:text-discord-text active:cursor-grabbing"
                      draggable
                      onDragStart={(e) => handleDragStart(e, p.id)}
                      onDragEnd={handleDragEnd}
                      title="Drag to reorder"
                    >
                      <IconGrip className="w-4 h-4" />
                    </td>
                    <td className="px-4 py-3 font-medium text-discord-text">{p.name}</td>
                    <td className="px-4 py-3 text-discord-textMuted">
                      {p.accountNumber} {p.label && <span className="text-discord-textMuted/80">· {p.label}</span>}
                    </td>
                    <td className="px-4 py-3">
                      <StatusBadge status={p.status} />
                    </td>
                    <td className="px-4 py-3 text-discord-textMuted whitespace-nowrap">{formatTimeRemaining(p.timeRemainingSeconds)}</td>
                    <td className="px-4 py-3 text-discord-textMuted whitespace-nowrap">{p.expiresAtPst ?? '—'}</td>
                    <td className="px-4 py-3 whitespace-nowrap text-right">
                      <div className="flex items-center justify-end gap-1">
                        <button
                          onClick={() => handleRefresh(p.id)}
                          disabled={refreshingIds.size > 0}
                          className="inline-flex items-center gap-1.5 rounded-md p-2 text-discord-textMuted hover:bg-discord-accent hover:text-white transition disabled:opacity-50"
                          title="Refresh credentials"
                        >
                          <IconRefresh className={`w-4 h-4 ${refreshingIds.has(p.id) ? 'animate-spin' : ''}`} />
                        </button>
                        <button
                          onClick={() => startEdit(p)}
                          className="inline-flex items-center gap-1.5 rounded-md p-2 text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition"
                          title="Edit profile"
                        >
                          <IconPencil className="w-4 h-4" />
                        </button>
                        <button
                          onClick={() => setDeleteConfirm({ id: p.id, name: p.name })}
                          className="inline-flex items-center gap-1.5 rounded-md p-2 text-discord-textMuted hover:bg-discord-danger/20 hover:text-discord-danger transition"
                          title="Delete profile"
                        >
                          <IconTrash className="w-4 h-4" />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {editing && (
          <div className="border-t border-discord-darkest p-6">
            <h3 className="text-lg font-medium text-discord-text mb-4">
              {dashboardProfiles.some((p) => p.id === editing.id) ? 'Edit profile' : 'New profile'}
            </h3>
            <div className="grid gap-4 sm:grid-cols-2">
              <div>
                <label className="block text-sm font-medium text-discord-textMuted">Profile name <span className="text-discord-danger">*</span></label>
                <input
                  value={form.name}
                  onChange={(e) => {
                    const name = e.target.value;
                    setForm((f) => ({
                      ...f,
                      name,
                      credentialProfileName:
                        f.credentialProfileName === f.name || f.credentialProfileName === '' ? name : f.credentialProfileName,
                    }));
                  }}
                  className="mt-1 w-full rounded-lg border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
                  placeholder="e.g. saml"
                />
              </div>
              <div>
                <label className="block text-sm font-medium text-discord-textMuted">Credentials section name <span className="text-discord-danger">*</span></label>
                <input
                  value={form.credentialProfileName}
                  onChange={(e) => setForm((f) => ({ ...f, credentialProfileName: e.target.value }))}
                  className="mt-1 w-full rounded-lg border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
                  placeholder="Same as profile name"
                />
              </div>
              <div className="sm:col-span-2">
                <label className="block text-sm font-medium text-discord-textMuted">IdP entry URL <span className="text-discord-danger">*</span></label>
                <input
                  value={form.idpEntryUrl}
                  onChange={(e) => setForm((f) => ({ ...f, idpEntryUrl: e.target.value }))}
                  className="mt-1 w-full rounded-lg border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
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
                    className="flex-1 rounded-lg border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text focus:border-discord-accent focus:outline-none"
                  >
                    <option value="">Select a role…</option>
                    {rolesForIdp?.map((r) => (
                      <option key={r.roleArn} value={r.roleArn}>
                        {roleToDisplayText(r, accountDisplayNames)}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    onClick={handleRefreshRoles}
                    disabled={loadingRoles || !form.idpEntryUrl?.trim()}
                    className="inline-flex items-center justify-center rounded-lg border border-discord-darkest bg-discord-darkest p-2 text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition disabled:opacity-50"
                    title="Load or refresh role list"
                  >
                    <IconRefresh className={`w-5 h-5 ${loadingRoles ? 'animate-spin' : ''}`} />
                  </button>
                </div>
              </div>
              <div>
                <label className="block text-sm font-medium text-discord-textMuted">Friendly label <span className="text-discord-danger">*</span></label>
                <input
                  value={form.label}
                  onChange={(e) => setForm((f) => ({ ...f, label: e.target.value }))}
                  className="mt-1 w-full rounded-lg border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text"
                  placeholder="e.g. Production"
                />
              </div>
              <div className="flex items-center gap-4 sm:col-span-2">
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={form.autoRefresh}
                    onChange={(e) => setForm((f) => ({ ...f, autoRefresh: e.target.checked }))}
                    className="rounded border-discord-darkest"
                  />
                  <span className="text-sm text-discord-textMuted">Auto refresh</span>
                </label>
                <div className="flex items-center gap-2">
                  <input
                    type="number"
                    min={1}
                    max={24}
                    value={form.refreshIntervalHours}
                    onChange={(e) =>
                      setForm((f) => ({ ...f, refreshIntervalHours: parseInt(e.target.value, 10) || 1 }))
                    }
                    className="w-16 rounded-lg border border-discord-darkest bg-discord-darkest px-2 py-1 text-discord-text"
                  />
                  <span className="text-sm text-discord-textMuted">hours</span>
                </div>
              </div>
              <div className="sm:col-span-2">
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={form.useDefaultCredentials ?? false}
                    onChange={(e) => setForm((f) => ({ ...f, useDefaultCredentials: e.target.checked }))}
                    className="rounded border-discord-darkest"
                  />
                  <span className="text-sm text-discord-textMuted">Use default credentials</span>
                </label>
                <p className="mt-1 text-xs text-discord-textMuted">
                  When on, refresh uses the username/password from Settings → Default credentials. When off, you are
                  prompted for username and password every time you refresh.
                </p>
              </div>
            </div>
            <div className="mt-6 flex gap-2">
              <button
                onClick={save}
                disabled={!requiredFieldsValid}
                className="inline-flex items-center gap-2 rounded-lg bg-discord-accent px-4 py-2.5 text-sm font-medium text-white hover:bg-discord-accentHover disabled:opacity-50 disabled:cursor-not-allowed"
              >
                Save
              </button>
              <button
                onClick={() => {
                  setEditing(null);
                  setFetchRolesModal(null);
                }}
                className="rounded-lg border border-discord-darkest bg-discord-darkest px-4 py-2.5 text-sm text-discord-textMuted hover:bg-discord-dark"
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
            setRefreshingIds((s) => { const n = new Set(s); n.delete(credentialsModal); return n; });
          }}
          onSubmit={handleSubmitCredentials}
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
      {deleteConfirm && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
          onClick={() => setDeleteConfirm(null)}
        >
          <div
            className="w-full max-w-md rounded-xl bg-discord-panel p-6 shadow-xl ring-1 ring-discord-darkest"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold text-discord-text">Delete profile</h3>
            <p className="mt-2 text-sm text-discord-textMuted">
              Delete &quot;{deleteConfirm.name}&quot;? This will also remove its credentials from the credentials file.
            </p>
            <div className="mt-6 flex justify-end gap-2">
              <button
                onClick={() => setDeleteConfirm(null)}
                className="rounded-lg border border-discord-darkest bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark"
              >
                Cancel
              </button>
              <button
                onClick={() => deleteConfirm && remove(deleteConfirm.id)}
                className="rounded-lg bg-discord-danger px-4 py-2 text-sm font-medium text-white hover:bg-discord-danger/90"
              >
                Delete
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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60" onClick={onClose}>
      <div className="w-full max-w-lg rounded-xl bg-discord-panel p-6 shadow-xl ring-1 ring-discord-darkest" onClick={(e) => e.stopPropagation()}>
        <h3 className="text-lg font-semibold text-discord-text">Choose role for {profileName}</h3>
        <p className="mt-1 text-sm text-discord-textMuted">Select which AWS role to assume.</p>
        <ul className="mt-4 space-y-2">
          {roles.map((r, i) => (
            <li key={i}>
              <button
                onClick={() => onSelect(i)}
                className="w-full rounded-lg border border-discord-darkest bg-discord-darkest px-3 py-2.5 text-left text-sm text-discord-text hover:bg-discord-dark transition"
              >
                {r.displayText}
              </button>
            </li>
          ))}
        </ul>
        <div className="mt-4">
          <button onClick={onClose} className="rounded-lg bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark">
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
  onSubmit: (profileId: string, username: string, password: string) => void;
}) {
  const [username, setUsername] = useState(initialUsername);
  const [password, setPassword] = useState('');
  useEffect(() => {
    setUsername(initialUsername);
  }, [initialUsername]);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60" onClick={onClose}>
      <div className="w-full max-w-md rounded-xl bg-discord-panel p-6 shadow-xl ring-1 ring-discord-darkest" onClick={(e) => e.stopPropagation()}>
        <h3 className="text-lg font-semibold text-discord-text">{title ?? `Sign in for ${profileName}`}</h3>
        <p className="mt-1 text-sm text-discord-textMuted">
          {description ?? 'Enter your IdP username and password. When using default credentials, they are stored in Windows Credential Manager.'}
        </p>
        <div className="mt-4 space-y-3">
          <input
            type="text"
            placeholder="Username"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            className="w-full rounded-lg border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
          />
          <input
            type="password"
            placeholder="Password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full rounded-lg border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
          />
        </div>
        <div className="mt-6 flex justify-end gap-2">
          <button onClick={onClose} className="rounded-lg bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark">
            Cancel
          </button>
          <button
            onClick={() => onSubmit(profileId, username, password)}
            disabled={!username || !password}
            className="rounded-lg bg-discord-accent px-4 py-2 text-sm font-medium text-white hover:bg-discord-accentHover disabled:opacity-50"
          >
            Sign in
          </button>
        </div>
      </div>
    </div>
  );
}
