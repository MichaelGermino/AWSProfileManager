/**
 * Master password validation. Aligns with NIST SP 800-63: minimum length 8,
 * allow long passphrases; no composition rules.
 */

export const MASTER_PASSWORD_MIN_LENGTH = 8;
/** Maximum length we accept (NIST allows up to 64; we allow 128 for passphrases). */
export const MASTER_PASSWORD_MAX_LENGTH = 128;

/** Human-readable requirements for the create-password UI. */
export const MASTER_PASSWORD_REQUIREMENTS =
  'Use at least 8 characters. Longer passphrases are stronger.';

/** Length in characters (Unicode code points), the unit every master-password rule uses. */
export function masterPasswordLength(password: string): number {
  return Array.from(password).length;
}

/**
 * Validates a master password. Returns an error message if invalid, or null if valid.
 */
export function validateMasterPassword(password: string): string | null {
  // Count characters (Unicode code points), not UTF-16 units: "🔑" is one character, so four of
  // them must not satisfy an 8-character minimum. Same count in the Rust port (chars().count()).
  const length = masterPasswordLength(password);
  if (length < MASTER_PASSWORD_MIN_LENGTH) {
    return `Password must be at least ${MASTER_PASSWORD_MIN_LENGTH} characters.`;
  }
  if (length > MASTER_PASSWORD_MAX_LENGTH) {
    return `Password must be no more than ${MASTER_PASSWORD_MAX_LENGTH} characters.`;
  }
  return null;
}
