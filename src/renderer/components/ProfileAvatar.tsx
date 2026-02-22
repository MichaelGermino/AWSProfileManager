import {
  PROFILE_ICONS,
  DEFAULT_PROFILE_ICON_ID,
  DEFAULT_ICON_COLOR,
} from '../data/profileIcons';

export interface ProfileAvatarProps {
  iconName?: string;
  iconColor?: string;
  className?: string;
  iconClassName?: string;
}

export function ProfileAvatar({
  iconName,
  iconColor,
  className = 'w-12 h-12 rounded-xl bg-discord-panel border border-discord-border flex items-center justify-center overflow-hidden',
  iconClassName = 'w-6 h-6',
}: ProfileAvatarProps) {
  const id = iconName && PROFILE_ICONS.some((i) => i.id === iconName) ? iconName : DEFAULT_PROFILE_ICON_ID;
  const icon = PROFILE_ICONS.find((i) => i.id === id) ?? PROFILE_ICONS[0];
  const color = iconColor && /^#[0-9A-Fa-f]{6}$/.test(iconColor) ? iconColor : DEFAULT_ICON_COLOR;

  return (
    <div className={className} aria-hidden>
      <svg
        className={iconClassName}
        fill="none"
        stroke={color}
        viewBox="0 0 24 24"
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {icon.paths && icon.paths.length > 0
          ? icon.paths.map((d, i) => <path key={i} d={d} />)
          : <path d={icon.path} />}
      </svg>
    </div>
  );
}
