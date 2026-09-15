/**
 * Guided feature tours, shown once after an update.
 *
 * The steps live in the renderer because every one of them is a route plus a DOM selector, and main
 * has no business knowing about either. Main only records which tour ids have been shown; see
 * `src/main/services/tourService.ts`.
 *
 * **Anchors are `data-tour` attributes, never classes or aria-labels.** A class is a styling
 * decision and an aria-label is user-visible copy — both get changed by someone with no reason to
 * suspect a tour depends on them, and a tour that silently loses its anchor is worse than no tour.
 * `data-tour` exists for nothing else, so it survives redesigns and greps honestly.
 *
 * **Adding a tour for a future release**, start to finish:
 *   1. Put `data-tour="<name>"` on each control you want to point at.
 *   2. Append a `Tour` to `TOURS` below with a new `id`.
 *   3. Run `FORCE_WHATS_NEW=1 npm run dev` and walk it. In that mode a step whose anchor is missing
 *      says so on the card instead of skipping, which is how you catch a selector that has rotted.
 *
 * **The `id` is the only thing that decides who has already seen a tour** — `settings.seenTourIds`
 * holds ids, not versions. So changing it re-shows the tour to everyone who saw the old one. That
 * is a real mechanism, not just a hazard: bumping the id (e.g. per RC while testing) is how you
 * deliberately put an updated tour back in front of people who already walked the previous one.
 * Just never change it *casually* — an edit for tidiness replays the tour for your whole user base.
 * Old ids left behind in `seenTourIds` are harmless; unknown ids are ignored.
 */

export interface TourStep {
  /** Stable within a tour; used as a React key and in the "skipped" logs. */
  id: string;
  title: string;
  body: string;
  /**
   * Selectors whose combined bounding box is the spotlight. Several because a control and the menu
   * it opens are separate elements (the menu is absolutely positioned, so the trigger's own rect
   * does not contain it). Omit entirely for a centered card with no highlight.
   */
  targets?: string[];
  /** Route to switch to before measuring. The panes are `display:none` when inactive, so an
   *  element on another route has no box to point at until we navigate. */
  route?: string;
  /** Opens whatever has to be open for the target to exist. Must be idempotent — the step is
   *  re-entered when the user walks back. */
  before?: () => void;
  /** Undoes `before`. Runs when the step is left in either direction, and on tour exit. */
  after?: () => void;
  /**
   * When the target never appears, skip this step rather than showing an unanchored card. Set for
   * anything conditional on app state — the folder button only exists once there is a profile.
   */
  skipIfMissing?: boolean;
  /** Preferred side; falls back automatically when it does not fit. Default 'auto'. */
  placement?: 'auto' | 'top' | 'bottom' | 'left' | 'right';
}

export interface Tour {
  id: string;
  /** Shown in the final step, purely for the user's orientation. */
  version: string;
  steps: TourStep[];
}

const openAddMenu = () => window.dispatchEvent(new Event('profiles:openAddMenu'));
const closeAddMenu = () => window.dispatchEvent(new Event('profiles:closeAddMenu'));
const openSettingsTab = (tab: string) =>
  window.dispatchEvent(new CustomEvent('settings:openTab', { detail: { tab } }));

/**
 * The console button lives on a profile row, and every folder may be collapsed — in which case
 * there is no row on screen to point at. Only override collapse when nothing is visible, so a user
 * whose profiles are already showing sees no folders fly open.
 */
const revealProfilesIfHidden = () => {
  if (!document.querySelector('[data-tour="open-console"]')) {
    window.dispatchEvent(new Event('profiles:revealProfiles'));
  }
};
/** Always safe: a no-op when nothing was revealed. */
const restoreFolders = () => window.dispatchEvent(new Event('profiles:restoreFolders'));

const V1_5_TOUR: Tour = {
  id: 'v1.5-features-2',
  version: '1.5.0',
  steps: [
    {
      id: 'welcome',
      title: "What's new in this version",
      body: 'A few things worth two minutes — where the newest features live and what they do. You can leave at any time with Escape.',
    },
    {
      id: 'bulk-import',
      title: 'Import accounts in bulk',
      body: 'Add accounts… signs you in once and then lets you pick which accounts and permission sets become profiles — instead of adding them one at a time. It works for both SAML and IAM Identity Center.',
      route: '/',
      targets: ['[data-tour="add-profile"]', '[data-tour="add-accounts-item"]'],
      before: openAddMenu,
      after: closeAddMenu,
      placement: 'left',
    },
    {
      id: 'folders',
      title: 'Group profiles into folders',
      body: 'Make a folder here, then drag profiles in — or right-click any profile and choose Move to folder, which is far easier across a long list. Folders are for your eyes only: they never change a profile name or how credentials are written.',
      route: '/',
      targets: ['[data-tour="new-folder"]'],
      // Gone when the list is empty, and a brand-new install has nothing to group.
      skipIfMissing: true,
      placement: 'bottom',
    },
    {
      id: 'one-click-console',
      title: 'Open the AWS console in one click',
      body: 'This opens the AWS console already signed in as that profile — no copying credentials, no sign-in page. Expired credentials are refreshed for you first, and several accounts can be signed in at once. The same thing is in the tray menu under AWS Console.',
      route: '/',
      targets: ['[data-tour="open-console"]'],
      before: revealProfilesIfHidden,
      after: restoreFolders,
      // No button at all until there is at least one profile.
      skipIfMissing: true,
      placement: 'left',
    },
    {
      id: 'background',
      title: 'Give the app a background',
      body: 'Pick any MP4 or WebM and choose where it plays — the sign-in screen only, or behind the whole window with translucent panels over it. The blur slider trades motion for legibility.',
      route: '/settings',
      targets: ['[data-tour="background-settings"]'],
      before: () => openSettingsTab('appearance'),
      placement: 'top',
    },
    {
      id: 'done',
      title: "That's it",
      body: 'The full release notes are on the GitHub release, linked from the What’s new popup.',
    },
  ],
};

/**
 * The "what's new after an update" sequence, oldest first.
 *
 * Only tours that should fire automatically after an update belong here. A tour started on demand
 * — a "where is X?" from a help link, say — must NOT be added to this array, or it will ambush
 * everyone on their next update; export it on its own and start it by id.
 */
export const TOURS: Tour[] = [V1_5_TOUR];

export const LATEST_TOUR: Tour = TOURS[TOURS.length - 1];

/**
 * Resolve a tour for `tour:start`. The single seam for on-demand tours: when a standalone one is
 * added, this is the only place that has to learn about it.
 */
export function findTour(id: string): Tour | undefined {
  return TOURS.find((t) => t.id === id);
}
