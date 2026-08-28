/** Line icons, drawn to a 24px grid at 1.6 stroke so they sit evenly with text. */

type Props = { className?: string; style?: React.CSSProperties };

const base = {
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

export const IconBoard = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M9 4v16M15 4v16" />
  </svg>
);

export const IconPlay = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <path d="M7 4.5l12 7.5-12 7.5z" />
  </svg>
);

export const IconStop = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <rect x="6" y="6" width="12" height="12" rx="2" />
  </svg>
);

export const IconModels = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <circle cx="12" cy="6" r="2.5" />
    <circle cx="5.5" cy="17" r="2.5" />
    <circle cx="18.5" cy="17" r="2.5" />
    <path d="M10.4 7.9L7.1 14.6M13.6 7.9l3.3 6.7M8 17h8" />
  </svg>
);

export const IconSettings = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <circle cx="12" cy="12" r="3.2" />
    <path d="M19.3 14.6a1.5 1.5 0 0 0 .3 1.7l.1.1a1.8 1.8 0 1 1-2.6 2.6l-.1-.1a1.5 1.5 0 0 0-2.6 1.1v.2a1.8 1.8 0 1 1-3.6 0v-.1a1.5 1.5 0 0 0-2.7-1.1l-.1.1a1.8 1.8 0 1 1-2.6-2.6l.1-.1a1.5 1.5 0 0 0-1.1-2.6h-.2a1.8 1.8 0 1 1 0-3.6h.1a1.5 1.5 0 0 0 1.1-2.7l-.1-.1a1.8 1.8 0 1 1 2.6-2.6l.1.1a1.5 1.5 0 0 0 1.7.3h.1a1.5 1.5 0 0 0 .9-1.4v-.2a1.8 1.8 0 1 1 3.6 0v.1a1.5 1.5 0 0 0 2.6 1.1l.1-.1a1.8 1.8 0 1 1 2.6 2.6l-.1.1a1.5 1.5 0 0 0-.3 1.7v.1a1.5 1.5 0 0 0 1.4.9h.2a1.8 1.8 0 1 1 0 3.6h-.1a1.5 1.5 0 0 0-1.4.9z" />
  </svg>
);

export const IconFolder = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
  </svg>
);

export const IconBranch = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <circle cx="6.5" cy="5.5" r="2.2" />
    <circle cx="6.5" cy="18.5" r="2.2" />
    <circle cx="17.5" cy="8" r="2.2" />
    <path d="M6.5 7.7v8.6M17.5 10.2c0 3.4-2.6 4.6-5.4 5.2-1.9.4-3.3 1.1-3.3 2.6" />
  </svg>
);

export const IconChevron = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <path d="M9 6l6 6-6 6" />
  </svg>
);

export const IconCheck = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <path d="M4.5 12.5l5 5 10-11" />
  </svg>
);

export const IconAlert = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <path d="M12 8.5v5M12 17h.01" />
    <path d="M10.3 3.9L2.6 17.2A2 2 0 0 0 4.3 20.2h15.4a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" />
  </svg>
);

export const IconClose = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);

export const IconSun = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2.5v2M12 19.5v2M21.5 12h-2M4.5 12h-2M18.4 5.6l-1.4 1.4M7 17l-1.4 1.4M18.4 18.4L17 17M7 7L5.6 5.6" />
  </svg>
);

export const IconMoon = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <path d="M20 14.2A8.2 8.2 0 0 1 9.8 4a8.4 8.4 0 1 0 10.2 10.2z" />
  </svg>
);

export const IconSparks = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <path d="M12 3.5l1.7 4.3 4.3 1.7-4.3 1.7L12 15.5l-1.7-4.3L6 9.5l4.3-1.7z" />
    <path d="M18.5 15.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z" />
  </svg>
);

export const IconRefresh = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <path d="M20 11.5A8 8 0 0 0 6.2 6.6L4 8.8" />
    <path d="M4 4.5v4.3h4.3" />
    <path d="M4 12.5a8 8 0 0 0 13.8 4.9l2.2-2.2" />
    <path d="M20 19.5v-4.3h-4.3" />
  </svg>
);

export const IconClock = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 7.5V12l3 1.8" />
  </svg>
);

export const IconFileDiff = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <path d="M14 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8.5z" />
    <path d="M14 3.5v5h5" />
    <path d="M9.5 12.5h5M12 10v5M9.5 17.5h5" />
  </svg>
);

export const IconHistory = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <circle cx="12" cy="6" r="2.5" />
    <circle cx="12" cy="18" r="2.5" />
    <path d="M12 8.5v7" />
    <path d="M14.5 6h5M14.5 18h5" />
  </svg>
);

export const IconSearch = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <circle cx="10.5" cy="10.5" r="6.5" />
    <path d="M15.5 15.5L20 20" />
  </svg>
);

export const IconPanel = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <rect x="3" y="4.5" width="18" height="15" rx="2" />
    <path d="M14.5 4.5v15" />
  </svg>
);

export const IconCopy = ({ className = "size-4", style }: Props) => (
  <svg {...base} className={className} style={style}>
    <rect x="9" y="9" width="11" height="11" rx="2" />
    <path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h3" />
  </svg>
);
