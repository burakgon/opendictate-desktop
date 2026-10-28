import { ChevronDown, Check, Eye, EyeOff, Loader2 } from "lucide-react";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
  type TextareaHTMLAttributes,
} from "react";
import { isCopilotKey, prettyToken, splitAccelerator } from "../lib/keys";

export function cx(...classes: (string | false | null | undefined)[]) {
  return classes.filter(Boolean).join(" ");
}

// ---------------------------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------------------------

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "subtle";

export function Button({
  variant = "secondary",
  size = "md",
  loading,
  icon,
  className,
  children,
  disabled,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: "sm" | "md" | "lg";
  loading?: boolean;
  icon?: ReactNode;
}) {
  const base =
    "inline-flex items-center justify-center gap-2 font-medium rounded-xl transition-all duration-150 select-none disabled:opacity-50 disabled:pointer-events-none active:scale-[0.98] whitespace-nowrap";
  const sizes = { sm: "h-8 px-3 text-[13px]", md: "h-10 px-4 text-sm", lg: "h-12 px-6 text-[15px]" };
  const variants: Record<ButtonVariant, string> = {
    primary:
      "brand-gradient text-white shadow-[0_8px_24px_-8px_rgba(124,58,237,0.7)] hover:brightness-110 hover:shadow-[0_10px_30px_-8px_rgba(124,58,237,0.9)]",
    secondary: "bg-white/[0.07] text-white/90 border border-white/10 hover:bg-white/[0.11] hover:border-white/15",
    ghost: "text-white/70 hover:text-white hover:bg-white/[0.06]",
    subtle: "bg-brand-500/15 text-brand-300 hover:bg-brand-500/25",
    danger: "bg-rose-500/15 text-rose-300 border border-rose-500/20 hover:bg-rose-500/25",
  };
  return (
    <button className={cx(base, sizes[size], variants[variant], className)} disabled={disabled || loading} {...rest}>
      {loading ? <Loader2 className="size-4 animate-spin" /> : icon}
      {children}
    </button>
  );
}

export function IconButton({
  className,
  children,
  title,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { title: string }) {
  return (
    <button
      title={title}
      aria-label={title}
      className={cx(
        "inline-flex size-8 items-center justify-center rounded-lg text-white/55 transition hover:bg-white/[0.08] hover:text-white",
        className,
      )}
      {...rest}
    >
      {children}
    </button>
  );
}

// ---------------------------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------------------------

export function PageHeader({ title, description, actions }: { title: string; description?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-7 flex items-end justify-between gap-6">
      <div>
        <h1 className="text-[26px] font-semibold tracking-tight text-white">{title}</h1>
        {description && <p className="mt-1.5 max-w-2xl text-[14px] leading-relaxed text-white/50">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  );
}

export function Section({
  title,
  description,
  icon,
  children,
  actions,
  className,
}: {
  title?: string;
  description?: ReactNode;
  icon?: ReactNode;
  children: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <section className={cx("card mb-5 p-6 animate-fade-in", className)}>
      {(title || actions) && (
        <div className="mb-5 flex items-start justify-between gap-4">
          <div className="flex items-start gap-3">
            {icon && (
              <div className="mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-xl bg-white/[0.06] text-brand-300">
                {icon}
              </div>
            )}
            <div>
              {title && <h2 className="text-[15px] font-semibold text-white">{title}</h2>}
              {description && <p className="mt-1 text-[13px] leading-relaxed text-white/45">{description}</p>}
            </div>
          </div>
          {actions}
        </div>
      )}
      {children}
    </section>
  );
}

export function Row({
  label,
  description,
  children,
  className,
}: {
  label: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx("flex items-center justify-between gap-8 border-t border-white/[0.05] py-4 first:border-t-0 first:pt-0 last:pb-0", className)}>
      <div className="min-w-0">
        <div className="text-[14px] font-medium text-white/90">{label}</div>
        {description && <div className="mt-0.5 text-[12.5px] leading-relaxed text-white/45">{description}</div>}
      </div>
      <div className="flex shrink-0 items-center gap-2">{children}</div>
    </div>
  );
}

export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="block">
      <div className="mb-1.5 text-[12.5px] font-medium text-white/60">{label}</div>
      {children}
      {hint && <div className="mt-1.5 text-[12px] text-white/40">{hint}</div>}
    </label>
  );
}

// ---------------------------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------------------------

export const inputClass =
  "w-full h-10 rounded-xl bg-black/25 border border-white/10 px-3.5 text-[14px] text-white placeholder:text-white/25 outline-none transition focus:border-brand-400/60 focus:bg-black/35 focus:ring-4 focus:ring-brand-500/10";

export function Input({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={cx(inputClass, className)} spellCheck={false} {...rest} />;
}

export function SecretInput({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder?: string }) {
  const [show, setShow] = useState(false);
  return (
    <div className="relative">
      <input
        className={cx(inputClass, "pr-10 font-mono text-[13px]")}
        type={show ? "text" : "password"}
        value={value}
        placeholder={placeholder}
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => onChange(e.target.value)}
      />
      <button
        type="button"
        className="absolute inset-y-0 right-0 flex w-10 items-center justify-center text-white/35 hover:text-white/80"
        onClick={() => setShow((s) => !s)}
        title={show ? "Hide" : "Show"}
      >
        {show ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
      </button>
    </div>
  );
}

export function TextArea({ className, ...rest }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      className={cx(
        "w-full rounded-xl bg-black/25 border border-white/10 px-3.5 py-3 text-[13.5px] leading-relaxed text-white placeholder:text-white/25 outline-none transition focus:border-brand-400/60 focus:bg-black/35 focus:ring-4 focus:ring-brand-500/10",
        className,
      )}
      {...rest}
    />
  );
}

export function Toggle({ checked, onChange, disabled }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cx(
        "relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors duration-200 disabled:opacity-40",
        checked ? "brand-gradient" : "bg-white/12",
      )}
    >
      <span
        className={cx(
          "inline-block size-5 rounded-full bg-white shadow-md transition-transform duration-200",
          checked ? "translate-x-[22px]" : "translate-x-0.5",
        )}
      />
    </button>
  );
}

export function Segmented<T extends string>({
  value,
  onChange,
  options,
  size = "md",
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: ReactNode; title?: string }[];
  size?: "sm" | "md";
}) {
  return (
    <div className="inline-flex rounded-xl border border-white/10 bg-black/25 p-1">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          title={o.title}
          onClick={() => onChange(o.value)}
          className={cx(
            "rounded-lg font-medium transition-all",
            size === "sm" ? "px-2.5 py-1 text-[12px]" : "px-3.5 py-1.5 text-[13px]",
            value === o.value ? "bg-white/[0.12] text-white shadow-sm" : "text-white/50 hover:text-white/80",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export interface SelectOption<T extends string> {
  value: T;
  label: ReactNode;
  description?: ReactNode;
}

export function Select<T extends string>({
  value,
  onChange,
  options,
  className,
  placeholder,
}: {
  value: T;
  onChange: (v: T) => void;
  options: SelectOption<T>[];
  className?: string;
  placeholder?: string;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);
  const current = options.find((o) => o.value === value);
  return (
    <div ref={ref} className={cx("relative", className)}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className={cx(inputClass, "flex items-center justify-between gap-2 text-left", open && "border-brand-400/60")}
      >
        <span className={cx("truncate", !current && "text-white/30")}>{current?.label ?? placeholder ?? "Select…"}</span>
        <ChevronDown className={cx("size-4 shrink-0 text-white/40 transition", open && "rotate-180")} />
      </button>
      {open && (
        <div className="absolute z-50 mt-1.5 max-h-72 w-full min-w-56 overflow-y-auto rounded-xl border border-white/10 bg-ink-800/95 p-1 shadow-2xl shadow-black/60 backdrop-blur-xl scroll-area animate-fade-in">
          {options.map((o) => (
            <button
              key={o.value}
              type="button"
              onClick={() => {
                onChange(o.value);
                setOpen(false);
              }}
              className={cx(
                "flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-[13.5px] transition",
                o.value === value ? "bg-brand-500/15 text-white" : "text-white/75 hover:bg-white/[0.06] hover:text-white",
              )}
            >
              <div className="min-w-0 flex-1">
                <div className="truncate">{o.label}</div>
                {o.description && <div className="truncate text-[12px] text-white/40">{o.description}</div>}
              </div>
              {o.value === value && <Check className="size-4 shrink-0 text-brand-300" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function Slider({
  value,
  onChange,
  min,
  max,
  step,
  className,
}: {
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step: number;
  className?: string;
}) {
  const pct = ((value - min) / (max - min)) * 100;
  return (
    <input
      type="range"
      min={min}
      max={max}
      step={step}
      value={value}
      onChange={(e) => onChange(Number(e.target.value))}
      className={cx("h-1.5 w-40 cursor-pointer appearance-none rounded-full accent-brand-500", className)}
      style={{ background: `linear-gradient(90deg, #8b5cf6 ${pct}%, rgba(255,255,255,0.12) ${pct}%)` }}
    />
  );
}

export function Badge({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span className={cx("inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium", className ?? "bg-white/[0.08] text-white/60")}>
      {children}
    </span>
  );
}

export function Keycaps({ accel, os, size = "md" }: { accel: string; os?: string; size?: "sm" | "md" | "lg" }) {
  // The Copilot key on recent laptops reports itself as Shift+Super+F23.
  const parts = isCopilotKey(accel) ? ["Copilot"] : splitAccelerator(accel);
  const s = { sm: "text-[11px] px-1.5 py-0.5 min-w-6", md: "text-[12.5px] px-2 py-1 min-w-8", lg: "text-[15px] px-3 py-1.5 min-w-10" }[size];
  if (parts.length === 0) return <span className="text-white/30">Not set</span>;
  return (
    <span className="inline-flex items-center gap-1">
      {parts.map((p, i) => (
        <span key={i} className="inline-flex items-center gap-1">
          {i > 0 && <span className="text-white/30">+</span>}
          <kbd className={cx("kbd inline-flex items-center justify-center text-white/90", s)}>{prettyToken(p, os)}</kbd>
        </span>
      ))}
    </span>
  );
}

export function StatusDot({ ok, warn }: { ok: boolean; warn?: boolean }) {
  return (
    <span className="relative inline-flex size-2.5">
      <span
        className={cx(
          "absolute inset-0 rounded-full",
          ok ? "bg-emerald-400" : warn ? "bg-amber-400" : "bg-rose-400",
          ok && "animate-pulse",
        )}
      />
    </span>
  );
}

// ---------------------------------------------------------------------------------------------
// Toasts
// ---------------------------------------------------------------------------------------------

interface Toast {
  id: number;
  text: string;
  kind: "info" | "success" | "error";
}

const ToastCtx = createContext<(text: string, kind?: Toast["kind"]) => void>(() => {});

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const push = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-3), { id, text, kind }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), kind === "error" ? 6000 : 3000);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="pointer-events-none fixed bottom-6 left-1/2 z-[100] flex -translate-x-1/2 flex-col items-center gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={cx(
              "pointer-events-auto max-w-lg animate-fade-in rounded-xl border px-4 py-2.5 text-[13.5px] shadow-2xl backdrop-blur-xl",
              t.kind === "error" && "border-rose-500/30 bg-rose-950/80 text-rose-100",
              t.kind === "success" && "border-emerald-500/25 bg-emerald-950/80 text-emerald-100",
              t.kind === "info" && "border-white/10 bg-ink-800/90 text-white/90",
            )}
          >
            {t.text}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

export function useToast() {
  return useContext(ToastCtx);
}
