import { useId } from 'react';
import {
  MASTER_PASSWORD_MIN_LENGTH,
  masterPasswordLength,
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
  /** id for the "Master password" heading, so the dialog wrapper can point aria-labelledby at it. */
  titleId?: string;
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
  titleId,
}: CreateMasterPasswordModalProps) {
  const fieldId = useId();
  return (
    <>
      {/* Header: single icon + title and (when not acknowledged) explanation */}
      <div className="flex gap-4 items-start px-6 pt-6 pb-4">
        <div className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-button bg-ui-sunken border border-ui-border text-ui-textMuted">
          <IconLock className="h-5 w-5" />
        </div>
        <div className="min-w-0 flex-1">
          <h3 id={titleId} className="text-base font-medium text-ui-text">Master password</h3>
          {!acknowledged && (
            <p className="mt-2 text-sm text-ui-text leading-relaxed">
              <span className="font-medium text-ui-text">We require a master password</span> to encrypt your saved credentials. You’ll enter it once each time you start the Profile Manager.
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
                // Keyboard users hold Space or Enter. 1.x accepted only a pointer hold, so keyboard and
                // screen-reader users could not create a master password at all.
                onKeyDown={(e) => {
                  if ((e.key === ' ' || e.key === 'Enter') && !e.repeat) {
                    e.preventDefault();
                    onHoldStart();
                  }
                }}
                onKeyUp={(e) => {
                  if (e.key === ' ' || e.key === 'Enter') onHoldEnd();
                }}
                onBlur={onHoldEnd}
                aria-description="Press and hold for one second to confirm"
                className="w-full rounded-button bg-ui-primary px-5 py-3.5 text-sm font-medium text-ui-onPrimary hover:bg-ui-primary/90 active:scale-[0.98] transition-colors duration-200 relative overflow-hidden"
              >
                <span className="relative z-10">{holdProgress >= 100 ? 'Done' : 'Hold to confirm'}</span>
                <span className="absolute inset-0 rounded-button bg-ui-onPrimary/20 transition-none" style={{ width: `${holdProgress}%` }} aria-hidden />
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="rounded-button border border-ui-border bg-ui-sunken/60 p-4 space-y-4">
              <p className="text-xs text-ui-textMuted">
                {MASTER_PASSWORD_REQUIREMENTS}
              </p>
              <div className="space-y-4">
                <div>
                  <label htmlFor={`${fieldId}-password`} className="label">
                    Password
                  </label>
                  <input
                    id={`${fieldId}-password`}
                    type="password"
                    value={password}
                    onChange={(e) => onPasswordChange(e.target.value)}
                    className="input mt-1.5"
                    placeholder="Choose a password"
                    autoFocus
                  />
                  <p className={`mt-1.5 flex items-center gap-1.5 text-xs ${masterPasswordLength(password) >= MASTER_PASSWORD_MIN_LENGTH ? 'text-ui-success' : 'text-ui-textMuted'}`}>
                    {masterPasswordLength(password) >= MASTER_PASSWORD_MIN_LENGTH ? (
                      <>
                        <IconCheckCircle className="w-3.5 h-3.5 flex-shrink-0" />
                        {MASTER_PASSWORD_MIN_LENGTH}+ characters
                      </>
                    ) : (
                      <>At least {MASTER_PASSWORD_MIN_LENGTH} characters{password.length > 0 && ` (${masterPasswordLength(password)}/${MASTER_PASSWORD_MIN_LENGTH})`}</>
                    )}
                  </p>
                </div>
                <div>
                  <label htmlFor={`${fieldId}-confirm`} className="label">
                    Confirm
                  </label>
                  <input
                    id={`${fieldId}-confirm`}
                    type="password"
                    value={confirmPassword}
                    onChange={(e) => onConfirmPasswordChange(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && onSubmit()}
                    className="input mt-1.5"
                    placeholder="Re-enter password"
                  />
                  {confirmPassword.length > 0 && (
                    <p className={`mt-1.5 flex items-center gap-1.5 text-xs ${password === confirmPassword ? 'text-ui-success' : 'text-ui-danger'}`}>
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
              <p className="mt-3 text-sm text-ui-danger flex items-center gap-1.5">
                {error}
              </p>
            )}

            <div className={`mt-5 flex gap-2 ${showCancel ? 'justify-end' : 'flex-col'}`}>
              {showCancel && onCancel && (
                <button
                  type="button"
                  onClick={onCancel}
                  disabled={submitting}
                  className="rounded-button border border-ui-border bg-transparent px-4 py-2.5 text-sm font-medium text-ui-textMuted hover:bg-ui-sunken hover:text-ui-text transition-colors disabled:opacity-50"
                >
                  {cancelLabel}
                </button>
              )}
              <button
                type="button"
                onClick={onSubmit}
                disabled={submitting}
                className={`rounded-button bg-ui-primary px-5 py-2.5 text-sm font-medium text-ui-onPrimary hover:bg-ui-primary/90 transition-colors disabled:opacity-50 ${showCancel ? '' : 'w-full'}`}
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
