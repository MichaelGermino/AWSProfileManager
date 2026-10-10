#!/usr/bin/env bash
# Reset AWS Profile Manager to a first-run state on macOS, for testing setup/onboarding.
# The macOS counterpart of reset-app-state.ps1.
#
# The app stores data in TWO places, plus the login Keychain:
#
#   1. ~/Library/Application Support/AWSProfileManager   - profiles.json, settings.json,
#                                                          sso-sessions.json, ui-prefs.json,
#                                                          rolesCache.json, auth-audit-log.json
#                                                          (hardcoded name, shared by dev and packaged)
#   2. Electron userData                                 - Partitions/ (persist:sso-*, persist:console-*),
#                                                          aws-cli-docs-cache/, Local Storage, cookies
#      ~/Library/Application Support/aws-profile-manager    when run via npm run dev
#      ~/Library/Application Support/AWS Profile Manager    when packaged
#   3. Keychain                                          - service "AWSProfileManager" (IdP username+password)
#
# Close the app first: it rewrites settings.json on exit and would undo the reset.
# Does NOT touch ~/.aws/credentials — that file is shared with the AWS CLI and other tools.
#
# Usage:
#   scripts/reset-app-state.sh            # remove everything above
#   scripts/reset-app-state.sh --dry-run  # only print what would be removed
#   scripts/reset-app-state.sh --keep-credentials
set -euo pipefail

DRY_RUN=0
KEEP_CREDENTIALS=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    --keep-credentials) KEEP_CREDENTIALS=1 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

remove_if_present() {
  local target="$1" label="$2"
  if [ -e "$target" ]; then
    if [ "$DRY_RUN" = 1 ]; then echo "would remove $label: $target"; else rm -rf "$target"; echo "removed $label: $target"; fi
  else
    echo "not present ($label): $target"
  fi
}

SUPPORT="$HOME/Library/Application Support"
remove_if_present "$SUPPORT/AWSProfileManager" "app data"
remove_if_present "$SUPPORT/aws-profile-manager" "userData (dev build)"
remove_if_present "$SUPPORT/AWS Profile Manager" "userData (packaged build)"

if [ "$KEEP_CREDENTIALS" = 1 ]; then
  echo "keeping Keychain entries"
else
  # keytar stores under service name "AWSProfileManager"; one entry per account. Loop until
  # none are left since `security` deletes one match per call.
  while security find-generic-password -s AWSProfileManager >/dev/null 2>&1; do
    if [ "$DRY_RUN" = 1 ]; then echo "would remove Keychain entries for service AWSProfileManager"; break; fi
    security delete-generic-password -s AWSProfileManager >/dev/null 2>&1 || break
    echo "removed a Keychain entry for service AWSProfileManager"
  done
fi
echo "done"
