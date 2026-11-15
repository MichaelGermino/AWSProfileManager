import { useEffect, useRef, useState } from 'react';
import { useDroppable } from '@dnd-kit/core';
import { useSortable } from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import type { ProfileFolder } from '../../shared/types';
import { PRESET_ICON_COLORS, DEFAULT_ICON_COLOR } from '../data/profileIcons';
import { FloatingMenu, menuAnchorFor } from './FloatingMenu';
import { Tooltip } from './Tooltip';

/**
 * One collapsible folder on the Profiles page: a header (drag handle, colored folder glyph,
 * inline-renameable name, count, overflow menu) wrapping its profile rows.
 *
 * The droppable is the whole section, not just the body, so a COLLAPSED folder is still a valid
 * drop target — its body isn't rendered, and anchoring the droppable there would make dropping
 * into a collapsed folder impossible. Hovering one for SPRING_OPEN_MS expands it, so a drag can
 * also aim at a specific position inside.
 */

export const FOLDER_SORTABLE_PREFIX = 'folder:';
export const FOLDER_CONTAINER_PREFIX = 'container:';
/** Container id for profiles that belong to no folder. */
export const UNGROUPED_CONTAINER_ID = `${FOLDER_CONTAINER_PREFIX}__ungrouped__`;

/** Hover-to-expand delay. Long enough not to fire while dragging *past* a folder. */
const SPRING_OPEN_MS = 600;

const IconFolder = ({ className = 'w-5 h-5', color }: { className?: string; color: string }) => (
  <svg className={className} fill="none" stroke={color} viewBox="0 0 24 24" strokeWidth={1.8} aria-hidden>
    <path
      strokeLinecap="round"
      strokeLinejoin="round"
      d="M3 7a2 2 0 012-2h3.586a1 1 0 01.707.293l1.414 1.414a1 1 0 00.707.293H19a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2V7z"
    />
  </svg>
);

const IconChevron = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2.5} aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" d="M9 5l7 7-7 7" />
  </svg>
);

const IconGrip = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path d="M8 6a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0zm6-12a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0zm0 6a2 2 0 11-4 0 2 2 0 014 0z" />
  </svg>
);

const IconDots = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path d="M12 8a2 2 0 110-4 2 2 0 010 4zm0 6a2 2 0 110-4 2 2 0 010 4zm0 6a2 2 0 110-4 2 2 0 010 4z" />
  </svg>
);

export interface ProfileFolderSectionProps {
  folder: ProfileFolder;
  count: number;
  collapsed: boolean;
  /** Shown when the folder renders no rows; differs between "empty" and "no search matches". */
  emptyLabel?: string;
  onToggleCollapsed: () => void;
  onRename: (name: string) => void;
  onRecolor: (color: string) => void;
  onDelete: () => void;
  children: React.ReactNode;
}

export function ProfileFolderSection({
  folder,
  count,
  collapsed,
  emptyLabel = 'Empty — drag profiles here',
  onToggleCollapsed,
  onRename,
  onRecolor,
  onDelete,
  children,
}: ProfileFolderSectionProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [colorOpen, setColorOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draftName, setDraftName] = useState(folder.name);
  const menuButtonRef = useRef<HTMLButtonElement>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);

  const color = folder.color && /^#[0-9A-Fa-f]{6}$/.test(folder.color) ? folder.color : DEFAULT_ICON_COLOR;

  const {
    attributes,
    listeners,
    setActivatorNodeRef,
    setNodeRef: setSortableRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: `${FOLDER_SORTABLE_PREFIX}${folder.id}` });

  const { setNodeRef: setDroppableRef, isOver } = useDroppable({
    id: `${FOLDER_CONTAINER_PREFIX}${folder.id}`,
  });

  // Spring-load: hovering a collapsed folder mid-drag opens it.
  useEffect(() => {
    if (!isOver || !collapsed) return;
    const timer = setTimeout(onToggleCollapsed, SPRING_OPEN_MS);
    return () => clearTimeout(timer);
  }, [isOver, collapsed, onToggleCollapsed]);

  // Dismissal (outside click, Escape, scroll, resize) is handled by FloatingMenu.

  useEffect(() => {
    if (renaming) renameInputRef.current?.select();
  }, [renaming]);

  const commitRename = () => {
    setRenaming(false);
    const next = draftName.trim();
    // A blank rename is a no-op rather than an unnameable folder; restore what was there.
    if (!next || next === folder.name) {
      setDraftName(folder.name);
      return;
    }
    onRename(next);
  };

  return (
    <div
      ref={(node) => {
        setSortableRef(node);
        setDroppableRef(node);
      }}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={`rounded-card transition-colors duration-150 ease-ui ${isDragging ? 'opacity-50' : ''} ${
 isOver ? 'bg-ui-accent/5 ring-1 ring-ui-accent/60' : ''
 }`}
      role="group"
      aria-label={`${folder.name} folder`}
    >
      <div className="flex h-9 items-center gap-1.5 pl-1 pr-1">
        <Tooltip label="Drag to reorder folder" placement="above" align="left">
          {/* Focusable drag handle for pointer and keyboard (see the profile row handle). */}
          <span
            ref={setActivatorNodeRef}
            className="flex h-8 w-6 flex-shrink-0 cursor-grab items-center justify-center rounded text-ui-textFaint hover:text-ui-text active:cursor-grabbing"
            {...attributes}
            {...listeners}
            aria-label={`Reorder ${folder.name} folder`}
          >
            <IconGrip className="w-4 h-4" />
          </span>
        </Tooltip>

        <button
          type="button"
          onClick={onToggleCollapsed}
          className="btn-icon h-7 w-7"
          aria-expanded={!collapsed}
          aria-label={collapsed ? `Expand ${folder.name}` : `Collapse ${folder.name}`}
        >
          <IconChevron className={`h-4 w-4 transition-transform duration-150 ease-ui ${collapsed ? '' : 'rotate-90'}`} />
        </button>

        <IconFolder className="h-4 w-4 flex-shrink-0" color={color} />

        {renaming ? (
          <input
            ref={renameInputRef}
            value={draftName}
            maxLength={40}
            onChange={(e) => setDraftName(e.target.value)}
            onBlur={commitRename}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitRename();
              if (e.key === 'Escape') {
                setDraftName(folder.name);
                setRenaming(false);
              }
            }}
            className="input h-7 min-w-0 flex-1 px-2 font-medium"
            aria-label="Folder name"
          />
        ) : (
          <button
            type="button"
            onDoubleClick={() => {
              setDraftName(folder.name);
              setRenaming(true);
            }}
            onClick={onToggleCollapsed}
            className="min-w-0 flex-1 truncate text-left text-sm font-medium text-ui-text"
            title="Double-click to rename"
          >
            {folder.name}
          </button>
        )}

        <span className="mono-meta flex-shrink-0 tabular">
          {count}
        </span>

        <div className="flex-shrink-0">
          <button
            ref={menuButtonRef}
            type="button"
            onClick={() => {
              setColorOpen(false);
              setMenuOpen((o) => !o);
            }}
            className="btn-icon"
            aria-label={`Folder options for ${folder.name}`}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
          >
            <IconDots className="w-4 h-4" />
          </button>

          {menuOpen && (
            <FloatingMenu
              {...menuAnchorFor(menuButtonRef.current)}
              align="right"
              triggerRef={menuButtonRef}
              onClose={() => {
                setMenuOpen(false);
                setColorOpen(false);
              }}
              className="w-52"
            >
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  setDraftName(folder.name);
                  setRenaming(true);
                }}
                className="w-full rounded-button px-3 py-2 text-left text-sm text-ui-text hover:bg-ui-hover transition-colors"
              >
                Rename
              </button>
              <button
                type="button"
                role="menuitem"
                onClick={() => setColorOpen((o) => !o)}
                className="w-full rounded-button px-3 py-2 text-left text-sm text-ui-text hover:bg-ui-hover transition-colors"
              >
                Change color
              </button>
              {colorOpen && (
                <div className="grid grid-cols-6 gap-1.5 px-2 py-2">
                  {PRESET_ICON_COLORS.map((hex) => (
                    <button
                      key={hex}
                      type="button"
                      onClick={() => {
                        onRecolor(hex);
                        setColorOpen(false);
                        setMenuOpen(false);
                      }}
                      style={{ backgroundColor: hex }}
                      className={`h-6 w-6 rounded-md border-2 transition-colors ${
 hex.toLowerCase() === color.toLowerCase()
 ? 'border-ui-text'
 : 'border-transparent'
 }`}
                      aria-label={`Set folder color ${hex}`}
                    />
                  ))}
                </div>
              )}
              <div className="my-1 h-px bg-ui-border" role="separator" />
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  onDelete();
                }}
                className="w-full rounded-button px-3 py-2 text-left text-sm text-ui-danger hover:bg-ui-danger/10 transition-colors"
              >
                Delete folder
              </button>
            </FloatingMenu>
          )}
        </div>
      </div>

      {!collapsed && (
        <div className="pb-2 pl-8">
          {count === 0 ? (
            <div className="rounded-button border border-dashed border-ui-border px-4 py-4 text-center text-sm text-ui-textFaint">
              {emptyLabel}
            </div>
          ) : (
            children
          )}
        </div>
      )}
    </div>
  );
}
