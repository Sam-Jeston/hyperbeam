import { cn } from "../../lib/utils";

/** The Culture mark: a dish (the sandbox) holding one agent and its two helpers. */
export function BrandMark({ size = 24, className }: { size?: number; className?: string }) {
  return (
    <svg viewBox="0 0 40 40" width={size} height={size} className={cn("shrink-0", className)} aria-hidden>
      <circle cx="20" cy="20" r="16.5" fill="none" stroke="var(--primary)" strokeWidth="3" />
      <circle cx="14.5" cy="21" r="5.2" fill="var(--primary)" />
      <circle cx="25.5" cy="13.8" r="3.3" fill="var(--second)" />
      <circle cx="25.8" cy="26.4" r="3.3" fill="var(--second)" />
    </svg>
  );
}

export function Wordmark({ className }: { className?: string }) {
  return <span className={cn("font-display font-bold tracking-[-0.035em]", className)}>noemune</span>;
}
