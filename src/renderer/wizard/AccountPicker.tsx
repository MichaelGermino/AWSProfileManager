import { useMemo, useState } from 'react';
import { allRoleKeys, countRoles, type ImportableAccount } from './importModel';

/**
 * Checkbox tree of accounts and their roles, shared by the SAML and Identity Center importers.
 *
 * Both sources normalize to ImportableAccount, so this component knows nothing about SAML ARNs or
 * SSO permission sets — it only deals in role keys.
 */
export function AccountPicker({
  accounts,
  selected,
  onChange,
}: {
  accounts: ImportableAccount[];
  selected: Set<string>;
  onChange: (next: Set<string>) => void;
}) {
  const [query, setQuery] = useState('');

  const total = useMemo(() => countRoles(accounts), [accounts]);
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return accounts;
    return accounts
      .map((a) => {
        // Matching the account keeps all its roles; otherwise narrow to matching roles.
        const accountMatches =
          a.accountName.toLowerCase().includes(q) || a.accountId.includes(q);
        const roles = accountMatches
          ? a.roles
          : a.roles.filter((r) => r.roleName.toLowerCase().includes(q));
        return { ...a, roles };
      })
      .filter((a) => a.roles.length > 0);
  }, [accounts, query]);

  const toggle = (key: string, on: boolean) => {
    const next = new Set(selected);
    if (on) next.add(key);
    else next.delete(key);
    onChange(next);
  };

  const toggleAccount = (account: ImportableAccount, on: boolean) => {
    const next = new Set(selected);
    for (const r of account.roles) {
      if (on) next.add(r.key);
      else next.delete(r.key);
    }
    onChange(next);
  };

  if (accounts.length === 0) {
    return (
      <p className="rounded-card border border-discord-border bg-discord-darkest p-4 text-sm text-discord-textMuted">
        No accounts were returned. If that looks wrong, check that you signed in with the right
        account.
      </p>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-3">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={`Filter ${accounts.length} accounts…`}
          className="flex-1 rounded-button border border-discord-border bg-discord-darkest px-3 py-1.5 text-sm text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
        />
        <button
          type="button"
          onClick={() => onChange(allRoleKeys(accounts))}
          className="text-xs text-discord-accent hover:underline"
        >
          Select all
        </button>
        <button
          type="button"
          onClick={() => onChange(new Set())}
          className="text-xs text-discord-textMuted hover:text-discord-text"
        >
          Clear
        </button>
      </div>

      <div className="mt-3 min-h-0 flex-1 overflow-y-auto rounded-card border border-discord-border bg-discord-darkest p-3">
        {filtered.map((account) => {
          const selectedHere = account.roles.filter((r) => selected.has(r.key)).length;
          const allOn = selectedHere === account.roles.length && account.roles.length > 0;
          return (
            <div key={account.accountId} className="mb-3 last:mb-0">
              <label className="flex items-start gap-2">
                <input
                  type="checkbox"
                  checked={allOn}
                  // Partially-selected accounts show the indeterminate dash rather than unchecked.
                  ref={(el) => {
                    if (el) el.indeterminate = selectedHere > 0 && !allOn;
                  }}
                  onChange={(e) => toggleAccount(account, e.target.checked)}
                  className="mt-0.5 rounded border-discord-border text-discord-accent focus:ring-discord-accent"
                />
                <span className="min-w-0">
                  <span className="block truncate text-sm font-medium text-discord-text">
                    {account.accountName}
                  </span>
                  <span className="block text-xs text-discord-textMuted">{account.accountId}</span>
                </span>
              </label>
              <div className="mt-1.5 space-y-1 pl-7">
                {account.roles.map((role) => (
                  <label key={role.key} className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={selected.has(role.key)}
                      onChange={(e) => toggle(role.key, e.target.checked)}
                      className="rounded border-discord-border text-discord-accent focus:ring-discord-accent"
                    />
                    <span className="text-sm text-discord-textMuted">{role.roleName}</span>
                  </label>
                ))}
              </div>
            </div>
          );
        })}
        {filtered.length === 0 && (
          <p className="py-6 text-center text-sm text-discord-textMuted">
            Nothing matches “{query}”.
          </p>
        )}
      </div>

      <p className="mt-2 text-xs text-discord-textMuted">
        {selected.size} of {total} selected
      </p>
    </div>
  );
}
