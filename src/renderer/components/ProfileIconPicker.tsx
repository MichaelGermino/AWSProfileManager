import { useState, useMemo } from 'react';
import {
  PROFILE_ICONS,
  PRESET_ICON_COLORS,
  DEFAULT_PROFILE_ICON_ID,
  DEFAULT_ICON_COLOR,
} from '../data/profileIcons';
import { ProfileAvatar } from './ProfileAvatar';
import { Tooltip } from './Tooltip';

export interface ProfileIconPickerProps {
  iconName: string | undefined;
  iconColor: string | undefined;
  onChange: (iconName: string | undefined, iconColor: string | undefined) => void;
}

export function ProfileIconPicker({
  iconName,
  iconColor,
  onChange,
}: ProfileIconPickerProps) {
  const [search, setSearch] = useState('');
  const [customColorOpen, setCustomColorOpen] = useState(false);
  const effectiveIcon = iconName && PROFILE_ICONS.some((i) => i.id === iconName) ? iconName : DEFAULT_PROFILE_ICON_ID;
  const effectiveColor = iconColor && /^#[0-9A-Fa-f]{6}$/.test(iconColor) ? iconColor : DEFAULT_ICON_COLOR;
  const isCustomColor = iconColor && /^#[0-9A-Fa-f]{6}$/.test(iconColor) && !PRESET_ICON_COLORS.includes(iconColor.toLowerCase());

  const filteredIcons = useMemo(() => {
    if (!search.trim()) return PROFILE_ICONS;
    const q = search.trim().toLowerCase();
    return PROFILE_ICONS.filter(
      (icon) => icon.id.toLowerCase().includes(q) || icon.search.toLowerCase().includes(q)
    );
  }, [search]);

  const handleSelectIcon = (id: string) => {
    onChange(id === DEFAULT_PROFILE_ICON_ID ? undefined : id, effectiveColor === DEFAULT_ICON_COLOR ? undefined : effectiveColor);
  };

  const handleSelectPresetColor = (hex: string) => {
    setCustomColorOpen(false);
    onChange(effectiveIcon === DEFAULT_PROFILE_ICON_ID ? undefined : effectiveIcon, hex === DEFAULT_ICON_COLOR ? undefined : hex);
  };

  const handleCustomColor = (e: React.ChangeEvent<HTMLInputElement>) => {
    const v = e.target.value;
    if (v && /^#[0-9A-Fa-f]{6}$/.test(v)) {
      onChange(effectiveIcon === DEFAULT_PROFILE_ICON_ID ? undefined : effectiveIcon, v);
    }
  };

  const isPresetSelected = PRESET_ICON_COLORS.includes(effectiveColor.toLowerCase());
  const isCustomColorSelected = effectiveColor && !isPresetSelected && /^#[0-9A-Fa-f]{6}$/.test(effectiveColor);

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-4">
        <ProfileAvatar
          iconName={effectiveIcon}
          iconColor={effectiveColor}
          className="w-14 h-14 rounded-xl bg-discord-panel border border-discord-border flex items-center justify-center overflow-hidden flex-shrink-0"
          iconClassName="w-7 h-7"
        />
        <div className="min-w-0">
          <p className="text-sm font-medium text-discord-text">Preview</p>
          <p className="text-xs text-discord-textMuted mt-0.5">Icon and color shown on profile list</p>
        </div>
      </div>

      <div>
        <label className="block text-sm font-medium text-discord-textMuted mb-2">Icon</label>
        <input
          type="search"
          placeholder="Search icons..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text text-sm placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
          aria-label="Search icons"
        />
        <div className="mt-2 grid grid-cols-6 sm:grid-cols-8 gap-1.5 max-h-44 overflow-y-auto rounded-button border border-discord-border bg-discord-darkest/50 p-2">
          {filteredIcons.map((icon) => (
            <button
              key={icon.id}
              type="button"
              onClick={() => handleSelectIcon(icon.id)}
              className={`flex items-center justify-center w-9 h-9 rounded-lg transition-colors ${
                effectiveIcon === icon.id
                  ? 'bg-discord-accent text-white ring-2 ring-discord-accent ring-offset-2 ring-offset-discord-darkest'
                  : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-text'
              }`}
              title={icon.id}
              aria-label={`Select ${icon.id} icon`}
              aria-pressed={effectiveIcon === icon.id}
            >
              <svg
                className="w-5 h-5"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                strokeWidth={1.5}
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                {icon.paths && icon.paths.length > 0
                  ? icon.paths.map((d, i) => <path key={i} d={d} />)
                  : <path d={icon.path} />}
              </svg>
            </button>
          ))}
        </div>
      </div>

      <div>
        <label className="block text-sm font-medium text-discord-textMuted mb-2">Color</label>
        <div className="flex flex-wrap items-center gap-2">
          {PRESET_ICON_COLORS.map((hex) => (
            <button
              key={hex}
              type="button"
              onClick={() => handleSelectPresetColor(hex)}
              className={`w-8 h-8 rounded-lg border-2 transition-all focus:outline-none focus:ring-2 focus:ring-discord-accent focus:ring-offset-2 focus:ring-offset-discord-darkest ${
                effectiveColor.toLowerCase() === hex.toLowerCase()
                  ? 'border-discord-text ring-2 ring-discord-accent ring-offset-2 ring-offset-discord-darkest scale-110'
                  : 'border-transparent hover:scale-105'
              }`}
              style={{ backgroundColor: hex }}
              title={hex}
              aria-label={`Color ${hex}`}
              aria-pressed={effectiveColor.toLowerCase() === hex.toLowerCase()}
            />
          ))}
          <div className="relative">
            <Tooltip label="Custom color">
              <button
                type="button"
                onClick={() => setCustomColorOpen((o) => !o)}
                className={`w-8 h-8 rounded-lg border-2 flex items-center justify-center transition-all focus:outline-none focus:ring-2 focus:ring-discord-accent focus:ring-offset-2 focus:ring-offset-discord-darkest ${
                  isCustomColorSelected
                    ? 'border-discord-text ring-2 ring-discord-accent ring-offset-2 ring-offset-discord-darkest'
                    : 'border-discord-border bg-discord-panel hover:border-discord-borderLight'
                }`}
                aria-label="Choose custom color"
                aria-expanded={customColorOpen}
              >
                <svg className="w-4 h-4 text-discord-textMuted" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.657-1.657a2 2 0 012.828 0l2.829 2.829a2 2 0 010 2.828l-8.486 8.485M7 17h.01" />
                </svg>
              </button>
            </Tooltip>
            {customColorOpen && (
              <div className="absolute left-0 top-full mt-1.5 z-10 flex items-center gap-2 rounded-button border border-discord-border bg-discord-panel p-2 shadow-discord-modal">
                <input
                  type="color"
                  value={effectiveColor}
                  onChange={handleCustomColor}
                  className="w-10 h-10 rounded cursor-pointer border-0 bg-transparent"
                  aria-label="Custom color"
                />
                <input
                  type="text"
                  value={customColorOpen ? (effectiveColor || '') : ''}
                  onChange={(e) => {
                    const raw = e.target.value;
                    const v = raw.startsWith('#') ? raw : raw ? `#${raw}` : '';
                    if (v === '' || /^#[0-9A-Fa-f]{0,6}$/.test(v)) {
                      if (v.length === 7) onChange(effectiveIcon === DEFAULT_PROFILE_ICON_ID ? undefined : effectiveIcon, v);
                    }
                  }}
                  placeholder="#000000"
                  className="w-24 rounded-button border border-discord-border bg-discord-darkest px-2 py-1.5 text-discord-text text-sm font-mono focus:border-discord-accent focus:outline-none"
                  aria-label="Hex color"
                />
              </div>
            )}
          </div>
        </div>
        {customColorOpen && (
          <p className="mt-1.5 text-xs text-discord-textMuted">Pick a color or enter a hex code (e.g. #3b82f6).</p>
        )}
      </div>
    </div>
  );
}
