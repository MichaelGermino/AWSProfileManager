import { useEffect, useState } from 'react';
import { HashRouter, Routes, Route, NavLink } from 'react-router-dom';
import Profiles from './pages/Profiles';
import Settings from './pages/Settings';

type UpdateStatus =
  | { type: 'available'; version: string }
  | { type: 'downloading'; percent: number }
  | { type: 'downloaded'; version: string }
  | { type: 'error'; message: string }
  | { type: 'no-update' };

function App() {
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);

  useEffect(() => {
    window.electron.onUpdateStatus((status: UpdateStatus) => setUpdateStatus(status));
  }, []);

  const handleInstallUpdate = () => {
    window.electron.installUpdateAndRestart();
  };

  const showUpdateBar = updateStatus?.type === 'downloaded' || updateStatus?.type === 'error';

  return (
    <HashRouter>
      <div className="flex h-full w-full bg-discord-darkest">
        <aside className="w-60 flex-shrink-0 bg-discord-sidebar flex flex-col py-3">
          <div className="px-4 py-2">
            <h1 className="text-lg font-semibold text-discord-text">AWS Profile Manager</h1>
          </div>
          <nav className="mt-4 flex-1 px-2">
            <NavLink
              to="/"
              className={({ isActive }) =>
                `flex items-center gap-3 rounded-md px-3 py-2.5 text-sm transition-colors ${
                  isActive ? 'bg-discord-accent text-white' : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-text'
                }`
              }
            >
              <svg className="h-5 w-5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
              </svg>
              Profiles
            </NavLink>
            <NavLink
              to="/settings"
              className={({ isActive }) =>
                `flex items-center gap-3 rounded-md px-3 py-2.5 text-sm transition-colors ${
                  isActive ? 'bg-discord-accent text-white' : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-text'
                }`
              }
            >
              <svg className="h-5 w-5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
              </svg>
              Settings
            </NavLink>
          </nav>
        </aside>
        <div className="flex-1 flex flex-col min-w-0">
          {showUpdateBar && (
            <header className="flex justify-end items-center gap-2 px-4 py-2 flex-shrink-0 bg-discord-darkest border-b border-discord-panel">
              {updateStatus?.type === 'downloaded' && (
                <button
                  type="button"
                  onClick={handleInstallUpdate}
                  title="Update Ready!"
                  className="rounded p-1.5 text-green-500 hover:bg-discord-panel hover:text-green-400 transition-colors"
                  aria-label="Update ready - click to install and restart"
                >
                  <svg className="h-6 w-6" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
                  </svg>
                </button>
              )}
              {updateStatus?.type === 'error' && (
                <span className="text-sm text-amber-400" title={updateStatus.message}>
                  Update check failed: {updateStatus.message}
                </span>
              )}
            </header>
          )}
          <main className="flex-1 overflow-auto p-6">
          <Routes>
            <Route path="/" element={<Profiles />} />
            <Route path="/settings" element={<Settings />} />
          </Routes>
          </main>
        </div>
      </div>
    </HashRouter>
  );
}

export default App;
