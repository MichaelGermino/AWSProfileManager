import { useEffect, useRef, useState } from 'react';
import { useDroppable } from '@dnd-kit/core';
import { useSortable } from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import type { ProfileFolder } from '../../shared/types';
import { PRESET_ICON_COLORS, DEFAULT_ICON_COLOR } from '../data/profileIcons';
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
  const menuRef = useRef<HTMLDivElement>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);

  const color = folder.color && /^#[0-9A-Fa-f]{6}$/.test(folder.color) ? folder.color : DEFAULT_ICON_COLOR;

  const {
    attributes,
    listeners,
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

  useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
        setColorOpen(false);
      }
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [menuOpen]);

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
      className={`rounded-card border transition-colors ${
        isDragging ? 'opacity-50' : ''
      } ${
        isOver
          ? 'border-discord-accent bg-discord-accent/5'
          : 'border-discord-border/70 bg-discord-darkest/30'
      }`}
      {...attributes}
    >
      <div className="flex items-center gap-2 px-2 py-2">
        <Tooltip label="Drag to reorder folder" placement="above" align="left">
          <span
            className="flex-shrink-0 p-1.5 cursor-grab text-discord-textMuted hover:text-discord-text active:cursor-grabbing rounded"
            {...listeners}
          >
            <IconGrip className="w-4 h-4" />
          </span>
        </Tooltip>

        <button
          type="button"
          onClick={onToggleCollapsed}
          className="flex-shrink-0 p-1 rounded text-discord-textMuted hover:text-discord-text hover:bg-discord-panel transition-colors"
          aria-expanded={!collapsed}
          aria-label={collapsed ? `Expand ${folder.name}` : `Collapse ${folder.name}`}
        >
          <IconChevron className={`w-4 h-4 transition-transform ${collapsed ? '' : 'rotate-90'}`} />
        </button>

        <IconFolder className="w-5 h-5 flex-shrink-0" color={color} />

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
            className="min-w-0 flex-1 rounded-button border border-discord-accent bg-discord-darkest px-2 py-1 text-sm font-semibold text-discord-text focus:outline-none"
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
            className="min-w-0 flex-1 truncate text-left text-sm font-semibold text-discord-text"
            title="Double-click to rename"
          >
            {folder.name}
          </button>
        )}

        <span className="flex-shrink-0 rounded-full bg-discord-darkest px-2 py-0.5 text-xs text-discord-textMuted">
          {count}
        </span>

        <div className="relative flex-shrink-0" ref={menuRef}>
          <button
            type="button"
            onClick={() => setMenuOpen((o) => !o)}
            className="rounded-button p-1.5 text-discord-textMuted hover:bg-discord-panel hover:text-discord-text transition-colors"
            aria-label={`Folder options for ${folder.name}`}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
          >
            <IconDots className="w-4 h-4" />
          </button>

          {menuOpen && (
            <div
              role="menu"
              className="absolute right-0 top-full z-20 mt-1 w-52 rounded-card border border-discord-border bg-discord-panel p-1 shadow-discord-modal"
            >
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  setDraftName(folder.name);
                  setRenaming(true);
                }}
                className="w-full rounded-button px-3 py-2 text-left text-sm text-discord-text hover:bg-discord-darkest transition-colors"
              >
                Rename
              </button>
              <button
                type="button"
                role="menuitem"
                onClick={() => setColorOpen((o) => !o)}
                className="w-full rounded-button px-3 py-2 text-left text-sm text-discord-text hover:bg-discord-darkest transition-colors"
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
                      className={`h-6 w-6 rounded-lg border-2 transition-transform hover:scale-110 ${
                        hex.toLowerCase() === color.toLowerCase()
                          ? 'border-discord-text'
                          : 'border-transparent'
                      }`}
                      aria-label={`Set folder color ${hex}`}
                    />
                  ))}
                </div>
              )}
              <div className="my-1 h-px bg-discord-border" role="separator" />
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  onDelete();
                }}
                className="w-full rounded-button px-3 py-2 text-left text-sm text-discord-danger hover:bg-discord-danger/10 transition-colors"
              >
                Delete folder
              </button>
            </div>
          )}
        </div>
      </div>

      {!collapsed && (
        <div className="px-2 pb-2">
          {count === 0 ? (
            <div className="rounded-card border border-dashed border-discord-border px-4 py-6 text-center text-xs text-discord-textMuted">
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
