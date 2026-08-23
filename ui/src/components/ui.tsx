/** Shared primitives. Everything visual is built from these. */

import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from "react";

export function cx(...values: (string | false | null | undefined)[]): string {
  return values.filter(Boolean).join(" ");
}

// --- Button ------------------------------------------------------------------

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
type ButtonSize = "sm" | "md";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: ReactNode;
}

const BUTTON_BASE =
  "inline-flex items-center justify-center gap-1.5 rounded-lg font-medium whitespace-nowrap " +
  "transition-[background,border-color,color,opacity] duration-150 " +
  "disabled:opacity-40 select-none";

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  className,
  children,
  ...rest
}: ButtonProps) {
  const sizing = size === "sm" ? "h-7 px-2.5 text-[12px]" : "h-8 px-3 text-[13px]";

  const styles: Record<ButtonVariant, string> = {
    primary:
      "text-white border border-transparent hover:brightness-110 active:brightness-95",
    secondary:
      "border hover:bg-[var(--surface-3)] active:bg-[var(--surface-2)]",
    ghost: "border border-transparent hover:bg-[var(--surface-3)]",
    danger:
      "border hover:bg-[var(--danger-soft)] border-transparent",
  };

  const inline: Record<ButtonVariant, React.CSSProperties> = {
    primary: { background: "var(--accent)" },
    secondary: {
      background: "var(--surface-2)",
      borderColor: "var(--border-strong)",
      color: "var(--text)",
    },
    ghost: { color: "var(--text-muted)" },
    danger: { color: "var(--danger)", background: "var(--danger-soft)" },
  };

  return (
    <button
      {...rest}
      style={{ ...inline[variant], ...rest.style }}
      className={cx(BUTTON_BASE, sizing, styles[variant], className)}
    >
      {icon}
      {children}
    </button>
  );
}

// --- Badge -------------------------------------------------------------------

type Tone = "neutral" | "accent" | "success" | "warn" | "danger" | "info";

const TONE_COLOURS: Record<Tone, { fg: string; bg: string }> = {
  neutral: { fg: "var(--text-muted)", bg: "var(--surface-3)" },
  accent: { fg: "var(--accent-text)", bg: "var(--accent-soft)" },
  success: { fg: "var(--success)", bg: "var(--success-soft)" },
  warn: { fg: "var(--warn)", bg: "var(--warn-soft)" },
  danger: { fg: "var(--danger)", bg: "var(--danger-soft)" },
  info: { fg: "var(--info)", bg: "var(--info-soft)" },
};

export function Badge({
  tone = "neutral",
  children,
  title,
  className,
}: {
  tone?: Tone;
  children: ReactNode;
  title?: string;
  className?: string;
}) {
  const { fg, bg } = TONE_COLOURS[tone];
  return (
    <span
      title={title}
      style={{ color: fg, background: bg }}
      className={cx(
        "inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium leading-tight",
        className,
      )}
    >
      {children}
    </span>
  );
}

export function Dot({ tone = "neutral", pulse }: { tone?: Tone; pulse?: boolean }) {
  return (
    <span
      style={{ background: TONE_COLOURS[tone].fg }}
      className={cx(
        "inline-block size-1.5 shrink-0 rounded-full align-middle",
        pulse && "running-dot",
      )}
    />
  );
}

// --- Toggle ------------------------------------------------------------------

export function Toggle({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label?: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      style={{
        background: checked ? "var(--accent)" : "var(--surface-3)",
        borderColor: checked ? "transparent" : "var(--border-strong)",
      }}
      className="relative h-[18px] w-8 shrink-0 rounded-full border transition-colors duration-200 disabled:opacity-40"
    >
      <span
        className="absolute top-1/2 size-3 -translate-y-1/2 rounded-full bg-white shadow transition-[left] duration-200"
        style={{ left: checked ? "calc(100% - 15px)" : "3px" }}
      />
    </button>
  );
}

// --- Inputs ------------------------------------------------------------------

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      style={{
        background: "var(--surface-2)",
        borderColor: "var(--border-strong)",
        color: "var(--text)",
        ...props.style,
      }}
      className={cx(
        "h-8 w-full rounded-lg border px-2.5 text-[13px] outline-none",
        "placeholder:text-[var(--text-faint)] focus:border-[var(--accent)]",
        props.className,
      )}
    />
  );
}

export function Select({
  value,
  onChange,
  options,
  placeholder,
  disabled,
  className,
}: {
  value: string;
  onChange: (value: string) => void;
  options: { value: string; label: string }[];
  placeholder?: string;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <select
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value)}
      style={{
        background: "var(--surface-2)",
        borderColor: "var(--border-strong)",
        color: "var(--text)",
      }}
      className={cx(
        "h-8 w-full rounded-lg border px-2 text-[13px] outline-none focus:border-[var(--accent)] disabled:opacity-40",
        className,
      )}
    >
      {placeholder && <option value="">{placeholder}</option>}
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-[12px] font-medium">{label}</span>
      {children}
      {hint && (
        <span className="text-[11.5px] leading-snug text-[var(--text-faint)]">
          {hint}
        </span>
      )}
    </label>
  );
}

// --- Layout ------------------------------------------------------------------

export function Panel({
  title,
  description,
  actions,
  children,
  className,
}: {
  title?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section
      style={{ background: "var(--surface)", boxShadow: "var(--shadow-panel)" }}
      className={cx("rounded-xl border", className)}
    >
      {(title || actions) && (
        <header className="flex items-start justify-between gap-4 border-b px-4 py-3">
          <div className="min-w-0">
            {title && <h2 className="text-[13.5px] font-semibold">{title}</h2>}
            {description && (
              <p className="mt-0.5 text-[12px] leading-snug text-[var(--text-muted)]">
                {description}
              </p>
            )}
          </div>
          {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
        </header>
      )}
      {children}
    </section>
  );
}

export function EmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon?: ReactNode;
  title: string;
  description?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 px-6 py-14 text-center">
      {icon && <div className="mb-1 text-[var(--text-faint)]">{icon}</div>}
      <p className="text-[13.5px] font-medium">{title}</p>
      {description && (
        <p className="max-w-sm text-[12.5px] leading-relaxed text-[var(--text-muted)]">
          {description}
        </p>
      )}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function Spinner({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={cx("size-3.5 animate-spin", className)}
      fill="none"
      aria-hidden
    >
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeOpacity="0.25" strokeWidth="3" />
      <path
        d="M21 12a9 9 0 0 0-9-9"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
      />
    </svg>
  );
}
