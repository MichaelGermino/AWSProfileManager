import {
  MASTER_PASSWORD_MIN_LENGTH,
  MASTER_PASSWORD_REQUIREMENTS,
} from '../../shared/masterPassword';

const IconLock = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 15v2m-6 4a2 2 0 01-2-2v-6a2 2 0 012-2h8a2 2 0 012 2v6a2 2 0 01-2 2H6zm0-8a2 2 0 01-2-2V7a2 2 0 012-2h8a2 2 0 012 2v2a2 2 0 01-2 2H6z" />
  </svg>
);
const IconCheckCircle = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>
);
const IconXCircle = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>
);

export interface CreateMasterPasswordModalProps {
  acknowledged: boolean;
  holdProgress: number;
  onHoldStart: () => void;
  onHoldEnd: () => void;
  password: string;
  onPasswordChange: (value: string) => void;
  confirmPassword: string;
  onConfirmPasswordChange: (value: string) => void;
  error: string;
  submitting: boolean;
  onSubmit: () => void;
  submitLabel: string;
  showCancel?: boolean;
  onCancel?: () => void;
  cancelLabel?: string;
}

export function CreateMasterPasswordModal({
  acknowledged,
  holdProgress,
  onHoldStart,
  onHoldEnd,
  password,
  onPasswordChange,
  confirmPassword,
  onConfirmPasswordChange,
  error,
  submitting,
  onSubmit,
  submitLabel,
  showCancel = false,
  onCancel,
  cancelLabel = 'Cancel',
}: CreateMasterPasswordModalProps) {
  return (
    <>
      {/* Header: single icon + title and (when not acknowledged) explanation */}
      <div className="flex gap-4 items-start px-6 pt-6 pb-4">
        <div className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-button bg-discord-darkest border border-discord-border text-discord-textMuted">
          <IconLock className="h-5 w-5" />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="text-base font-semibold text-discord-text">Master password</h3>
          {!acknowledged && (
            <p className="mt-2 text-sm text-discord-text leading-relaxed">
              <span className="text-discord-accent font-medium">We require a master password</span> to encrypt your saved credentials. You’ll enter it once each time you start the Profile Manager.
            </p>
          )}
        </div>
      </div>

      {/* Acknowledgment step or Form */}
      <div className="px-6 pb-6">
        {!acknowledged ? (
          <div className="pt-0">
            <div className="mt-1">
              <button
                type="button"
                onPointerDown={onHoldStart}
                onPointerUp={onHoldEnd}
                onPointerLeave={onHoldEnd}
                className="w-full rounded-button bg-discord-accent px-5 py-3.5 text-sm font-medium text-white hover:bg-discord-accentHover active:scale-[0.98] transition-all duration-200 relative overflow-hidden"
              >
                <span className="relative z-10">{holdProgress >= 100 ? 'Done' : 'Hold to Confirm'}</span>
                <span className="absolute inset-0 rounded-button bg-white/20 transition-none" style={{ width: `${holdProgress}%` }} aria-hidden />
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="rounded-button border border-discord-border bg-discord-darkest/60 p-4 space-y-4">
              <p className="text-xs text-discord-textMuted">
                {MASTER_PASSWORD_REQUIREMENTS}
              </p>
              <div className="space-y-4">
                <div>
                  <label className="block text-xs font-medium text-discord-textMuted uppercase tracking-wide mb-1.5">
                    Password
                  </label>
                  <input
                    type="password"
                    value={password}
                    onChange={(e) => onPasswordChange(e.target.value)}
                    className="w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2.5 text-sm text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                    placeholder="Choose a password"
                    autoFocus
                  />
                  <p className={`mt-1.5 flex items-center gap-1.5 text-xs ${password.length >= MASTER_PASSWORD_MIN_LENGTH ? 'text-discord-success' : 'text-discord-textMuted'}`}>
                    {password.length >= MASTER_PASSWORD_MIN_LENGTH ? (
                      <>
                        <IconCheckCircle className="w-3.5 h-3.5 flex-shrink-0" />
                        {MASTER_PASSWORD_MIN_LENGTH}+ characters
                      </>
                    ) : (
                      <>At least {MASTER_PASSWORD_MIN_LENGTH} characters{password.length > 0 && ` (${password.length}/${MASTER_PASSWORD_MIN_LENGTH})`}</>
                    )}
                  </p>
                </div>
                <div>
                  <label className="block text-xs font-medium text-discord-textMuted uppercase tracking-wide mb-1.5">
                    Confirm
                  </label>
                  <input
                    type="password"
                    value={confirmPassword}
                    onChange={(e) => onConfirmPasswordChange(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && onSubmit()}
                    className="w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2.5 text-sm text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                    placeholder="Re-enter password"
                  />
                  {confirmPassword.length > 0 && (
                    <p className={`mt-1.5 flex items-center gap-1.5 text-xs ${password === confirmPassword ? 'text-discord-success' : 'text-discord-danger'}`}>
                      {password === confirmPassword ? (
                        <>
                          <IconCheckCircle className="w-3.5 h-3.5 flex-shrink-0" />
                          Passwords match
                        </>
                      ) : (
                        <>
                          <IconXCircle className="w-3.5 h-3.5 flex-shrink-0" />
                          Passwords don&apos;t match
                        </>
                      )}
                    </p>
                  )}
                </div>
              </div>
            </div>

            {error && (
              <p className="mt-3 text-sm text-discord-danger flex items-center gap-1.5">
                {error}
              </p>
            )}

            <div className={`mt-5 flex gap-2 ${showCancel ? 'justify-end' : 'flex-col'}`}>
              {showCancel && onCancel && (
                <button
                  type="button"
                  onClick={onCancel}
                  disabled={submitting}
                  className="rounded-button border border-discord-border bg-transparent px-4 py-2.5 text-sm font-medium text-discord-textMuted hover:bg-discord-darkest hover:text-discord-text transition-colors disabled:opacity-50"
                >
                  {cancelLabel}
                </button>
              )}
              <button
                type="button"
                onClick={onSubmit}
                disabled={submitting}
                className={`rounded-button bg-discord-accent px-5 py-2.5 text-sm font-medium text-white hover:bg-discord-accentHover transition-colors disabled:opacity-50 ${showCancel ? '' : 'w-full'}`}
              >
                {submitting ? 'Please wait...' : submitLabel}
              </button>
            </div>
          </>
        )}
      </div>
    </>
  );
}
