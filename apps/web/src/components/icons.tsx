import type { SVGProps } from 'react';

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function base({ size = 16, ...props }: IconProps, path: React.ReactNode) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      {...props}
    >
      {path}
    </svg>
  );
}

export const SendIcon = (p: IconProps) =>
  base(p, <path d="M12 19V5M5 12l7-7 7 7" />);
export const StopIcon = (p: IconProps) =>
  base(p, <rect x="7" y="7" width="10" height="10" rx="1.5" />);
export const GearIcon = (p: IconProps) =>
  base(
    p,
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </>,
  );
export const XIcon = (p: IconProps) =>
  base(p, <path d="M18 6 6 18M6 6l12 12" />);
export const TrashIcon = (p: IconProps) =>
  base(
    p,
    <path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />,
  );
export const SparkIcon = (p: IconProps) =>
  base(
    p,
    <path d="M12 3v3m0 12v3M5.6 5.6l2.2 2.2m8.4 8.4 2.2 2.2M3 12h3m12 0h3M5.6 18.4l2.2-2.2m8.4-8.4 2.2-2.2" />,
  );
export const PulseIcon = (p: IconProps) =>
  base(p, <path d="M22 12h-4l-3 9L9 3l-3 9H2" />);
export const ClockIcon = (p: IconProps) =>
  base(
    p,
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </>,
  );
export const HistoryIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8M3 3v5h5" />
      <path d="M12 7v5l4 2" />
    </>,
  );
export const SearchIcon = (p: IconProps) =>
  base(
    p,
    <>
      <circle cx="11" cy="11" r="8" />
      <path d="m21 21-4.3-4.3" />
    </>,
  );
export const BoltIcon = (p: IconProps) =>
  base(p, <path d="M13 2 3 14h7l-1 8 10-12h-7l1-8z" />);
export const ChevronIcon = (p: IconProps) => base(p, <path d="m6 9 6 6 6-6" />);
export const AlertIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
      <path d="M12 9v4M12 17h.01" />
    </>,
  );
export const ChipIcon = (p: IconProps) =>
  base(
    p,
    <>
      <rect x="6" y="6" width="12" height="12" rx="2" />
      <path d="M9 2v2m6-2v2M9 20v2m6-2v2M2 9h2m-2 6h2m16-6h2m-2 6h2" />
    </>,
  );
export const PlusIcon = (p: IconProps) =>
  base(p, <path d="M12 5v14M5 12h14" />);
export const ShieldIcon = (p: IconProps) =>
  base(p, <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />);
export const AgentsIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
      <circle cx="9" cy="7" r="4" />
      <path d="M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75" />
    </>,
  );
export const LogIcon = (p: IconProps) =>
  base(p, <path d="M4 6h16M4 12h16M4 18h10" />);
export const HeartIcon = (p: IconProps) =>
  base(
    p,
    <path d="M20.8 5.6a5 5 0 0 0-7.1 0L12 7.3l-1.7-1.7a5 5 0 0 0-7.1 7.1L12 21l8.8-8.3a5 5 0 0 0 0-7.1z" />,
  );
export const InboxIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M22 12h-6l-2 3h-4l-2-3H2" />
      <path d="M5.5 5h13L22 12v6a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-6z" />
    </>,
  );
export const ChartIcon = (p: IconProps) =>
  base(p, <path d="M4 20V10M10 20V4M16 20v-7M22 20H2" />);
export const MicIcon = (p: IconProps) =>
  base(
    p,
    <>
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
    </>,
  );
export const PaperclipIcon = (p: IconProps) =>
  base(
    p,
    <path d="m21 11-8.6 8.6a5 5 0 0 1-7.1-7.1l8.6-8.6a3.3 3.3 0 0 1 4.7 4.7l-8.6 8.6a1.7 1.7 0 0 1-2.4-2.4L15.5 7" />,
  );

export const BellIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
      <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
    </>,
  );
export const CheckIcon = (p: IconProps) =>
  base(p, <path d="M20 6 9 17l-5-5" />);
export const HashIcon = (p: IconProps) =>
  base(p, <path d="M4 9h16M4 15h16M10 3 8 21M16 3l-2 18" />);
export const TelegramIcon = (p: IconProps) =>
  base(p, <path d="M22 2 11 13M22 2l-7 20-4-9-9-4z" />);
export const SnoozeIcon = (p: IconProps) =>
  base(
    p,
    <>
      <circle cx="12" cy="13" r="8" />
      <path d="M12 9v4l2 2M5 3 2 6M22 6l-3-3" />
    </>,
  );

export const UserRoundIcon = (p: IconProps) =>
  base(
    p,
    <>
      <circle cx="12" cy="8" r="5" />
      <path d="M20 21a8 8 0 0 0-16 0" />
    </>,
  );
export const ShieldCheckIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z" />
      <path d="m9 12 2 2 4-4" />
    </>,
  );
export const PuzzleIcon = (p: IconProps) =>
  base(
    p,
    <path d="M19.4 15a1 1 0 0 0 1.6.8H22v4a2 2 0 0 1-2 2h-4v-1a2 2 0 1 0-4 0v1H8a2 2 0 0 1-2-2v-4H5a2 2 0 1 1 0-4h1V8a2 2 0 0 1 2-2h4V5a2 2 0 1 1 4 0v1h4a2 2 0 0 1 2 2v4h-1a2 2 0 0 0-1.6 3Z" />,
  );
export const BrainIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M12 18V5a3 3 0 0 0-5.9-.7A4 4 0 0 0 4 11a4 4 0 0 0 1 7.5A3.5 3.5 0 0 0 12 18ZM12 5a3 3 0 0 1 5.9-.7A4 4 0 0 1 20 11a4 4 0 0 1-1 7.5A3.5 3.5 0 0 1 12 18" />
      <path d="M8 8a4 4 0 0 1-2-3M16 8a4 4 0 0 0 2-3M7 15a4 4 0 0 0 5-3M17 15a4 4 0 0 1-5-3" />
    </>,
  );
export const BlocksIcon = (p: IconProps) =>
  base(
    p,
    <>
      <rect x="14" y="3" width="7" height="7" rx="1" />
      <path d="M4 14h6v7H4a1 1 0 0 1-1-1v-5a1 1 0 0 1 1-1ZM14 14h7v6a1 1 0 0 1-1 1h-6ZM3 3h7v7H3Z" />
    </>,
  );
export const LinkIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M10 13a5 5 0 0 0 7 .2l3-3a5 5 0 0 0-7-7l-1.7 1.7M14 11a5 5 0 0 0-7-.2l-3 3a5 5 0 0 0 7 7l1.7-1.7" />
    </>,
  );
export const CalendarClockIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M16 2v4M8 2v4M3 10h18M21 11V6a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h6" />
      <circle cx="17" cy="17" r="5" />
      <path d="M17 14v3l2 1" />
    </>,
  );
export const ListChecksIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="m3 6 2 2 4-4M13 6h8M3 14l2 2 4-4M13 14h8M13 21h8" />
    </>,
  );
export const FolderIcon = (p: IconProps) =>
  base(
    p,
    <path d="M20 20H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2Z" />,
  );
export const ServerIcon = (p: IconProps) =>
  base(
    p,
    <>
      <rect x="2" y="3" width="20" height="7" rx="2" />
      <rect x="2" y="14" width="20" height="7" rx="2" />
      <path d="M6 6.5h.01M6 17.5h.01" />
    </>,
  );
export const ScrollTextIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M8 21h12a2 2 0 0 0 2-2v-2H10v2a2 2 0 0 1-4 0V5a2 2 0 0 0-4 0v3h4M4 3h13a2 2 0 0 1 2 2v12M10 7h5M10 11h5" />
    </>,
  );
export const HardDriveIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M22 12H2" />
      <path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z" />
      <path d="M6 16h.01M10 16h.01" />
    </>,
  );
export const KeyRoundIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z" />
      <circle cx="16.5" cy="7.5" r=".5" fill="currentColor" />
    </>,
  );
export const MoonIcon = (p: IconProps) =>
  base(p, <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" />);
export const SlidersIcon = (p: IconProps) =>
  base(
    p,
    <path d="M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4" />,
  );
export const RocketIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z" />
      <path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z" />
      <path d="M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5" />
    </>,
  );
export const EyeIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" />
      <circle cx="12" cy="12" r="3" />
    </>,
  );
export const PencilIcon = (p: IconProps) =>
  base(
    p,
    <>
      <path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" />
      <path d="m15 5 4 4" />
    </>,
  );
export const TerminalIcon = (p: IconProps) =>
  base(p, <path d="m4 17 6-6-6-6M12 19h8" />);
export const GlobeIcon = (p: IconProps) =>
  base(
    p,
    <>
      <circle cx="12" cy="12" r="10" />
      <path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20M2 12h20" />
    </>,
  );
export const ArrowRightIcon = (p: IconProps) =>
  base(p, <path d="M5 12h14m-7-7 7 7-7 7" />);
export const PlayIcon = (p: IconProps) =>
  base(p, <path d="M6 3 20 12 6 21Z" />);
export const PauseIcon = (p: IconProps) =>
  base(
    p,
    <>
      <rect x="14" y="4" width="4" height="16" rx="1" />
      <rect x="6" y="4" width="4" height="16" rx="1" />
    </>,
  );
export const SquareDotIcon = (p: IconProps) =>
  base(
    p,
    <>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <circle cx="12" cy="12" r="1" />
    </>,
  );
export const SquareIcon = (p: IconProps) =>
  base(p, <rect x="3" y="3" width="18" height="18" rx="2" />);
export const DownloadIcon = (p: IconProps) =>
  base(p, <path d="M12 3v12m-5-5 5 5 5-5M5 21h14" />);
export const RefreshIcon = (p: IconProps) =>
  base(
    p,
    <path d="M20 7v5h-5M4 17v-5h5M6.1 6.1A8 8 0 0 1 20 12M4 12a8 8 0 0 0 13.9 5.9" />,
  );
export const ExternalLinkIcon = (p: IconProps) =>
  base(
    p,
    <path d="M15 3h6v6m0-6L10 14M9 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-4" />,
  );
export const CodeIcon = (p: IconProps) =>
  base(p, <path d="m8 7-5 5 5 5m8-10 5 5-5 5m-3-13-2 20" />);
export const MaximizeIcon = (p: IconProps) =>
  base(p, <path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5" />);
export const MinimizeIcon = (p: IconProps) =>
  base(p, <path d="M3 8h5V3m8 0v5h5M3 16h5v5m8 0v-5h5" />);
export const AtSignIcon = (p: IconProps) =>
  base(
    p,
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-3.92 7.94" />
    </>,
  );
export const CopyIcon = (p: IconProps) =>
  base(
    p,
    <>
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </>,
  );
