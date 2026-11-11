// Cards, badges, notices, skeletons and empty states.

import { cva, type VariantProps } from "class-variance-authority";
import { CircleCheck, Info, TriangleAlert, CircleX } from "lucide-react";
import type { HTMLAttributes, ReactNode } from "react";
import { cn } from "../../lib/utils";

export function Card({ className, interactive, ...props }: HTMLAttributes<HTMLDivElement> & { interactive?: boolean }) {
  return (
    <div
      className={cn(
        "rounded-lg border border-border bg-surface",
        interactive && "transition-[border-color,box-shadow,transform] hover:border-border-strong hover:shadow-soft",
        className,
      )}
      {...props}
    />
  );
}

/** A titled card section, for side panels and settings groups. */
export function Panel(props: { title?: ReactNode; action?: ReactNode; children: ReactNode; className?: string; bodyClassName?: string; flush?: boolean }) {
  return (
    <Card className={props.className}>
      {(props.title || props.action) && (
        <div className="flex items-center justify-between gap-3 px-4 pt-3.5 pb-0">
          {props.title && <h3 className="text-[11.5px] font-semibold uppercase tracking-[0.07em] text-faint">{props.title}</h3>}
          {props.action}
        </div>
      )}
      <div className={cn(props.flush ? "pt-2" : "px-4 pt-2.5 pb-4", props.bodyClassName)}>{props.children}</div>
    </Card>
  );
}

export const badgeVariants = cva(
  "inline-flex h-5.5 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full px-2 text-[12px] font-medium [&_svg]:size-3.5",
  {
    variants: {
      tone: {
        neutral: "bg-surface-2 text-muted",
        outline: "border border-border-strong text-muted",
        primary: "bg-accent text-accent-foreground",
        run: "bg-run/14 text-run",
        wait: "bg-wait/14 text-wait",
        idle: "bg-idle/14 text-idle",
        fail: "bg-fail/14 text-fail",
      },
      mono: { true: "font-mono text-[11.5px]" },
    },
    defaultVariants: { tone: "neutral" },
  },
);

export function Badge({ className, tone, mono, ...props }: HTMLAttributes<HTMLSpanElement> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ tone, mono }), className)} {...props} />;
}

const NOTICE = {
  info: { icon: Info, cls: "border-idle/25 bg-idle/8 text-foreground [&>svg]:text-idle" },
  success: { icon: CircleCheck, cls: "border-run/25 bg-run/8 text-foreground [&>svg]:text-run" },
  warning: { icon: TriangleAlert, cls: "border-wait/30 bg-wait/8 text-foreground [&>svg]:text-wait" },
  error: { icon: CircleX, cls: "border-fail/30 bg-fail/8 text-foreground [&>svg]:text-fail" },
};

/** An inline message about the thing it sits next to. */
export function Notice(props: { tone?: keyof typeof NOTICE; title?: ReactNode; children?: ReactNode; className?: string; action?: ReactNode }) {
  const { icon: Icon, cls } = NOTICE[props.tone ?? "info"];
  return (
    <div role={props.tone === "error" ? "alert" : undefined} className={cn("flex gap-2.5 rounded-lg border px-3.5 py-2.5 text-[13px] leading-snug", cls, props.className)}>
      <Icon className="mt-px size-4 shrink-0" />
      <div className="grid min-w-0 flex-1 gap-0.5">
        {props.title && <div className="font-semibold">{props.title}</div>}
        {props.children && <div className="whitespace-pre-wrap text-muted [overflow-wrap:anywhere]">{props.children}</div>}
      </div>
      {props.action}
    </div>
  );
}

export function Skeleton({ className }: { className?: string }) {
  return <div className={cn("rounded-md bg-surface-2 animate-shimmer", className)} aria-hidden />;
}

/** What a page or list shows before it has anything in it. */
export function EmptyState(props: { icon?: ReactNode; title: ReactNode; children?: ReactNode; action?: ReactNode; className?: string; compact?: boolean }) {
  return (
    <div
      className={cn(
        "flex flex-col items-center rounded-lg border border-dashed border-border-strong text-center",
        props.compact ? "gap-1.5 px-4 py-6" : "gap-2 px-6 py-12",
        props.className,
      )}
    >
      {props.icon && (
        <div className="mb-1 grid size-10 place-items-center rounded-full bg-accent text-accent-foreground [&_svg]:size-5">{props.icon}</div>
      )}
      <div className="font-display text-[15px] font-semibold">{props.title}</div>
      {props.children && <div className="max-w-[52ch] text-[13.5px] text-muted">{props.children}</div>}
      {props.action && <div className="mt-2">{props.action}</div>}
    </div>
  );
}

/** Label/value pairs. */
export function KeyValues({ rows }: { rows: [ReactNode, ReactNode][] }) {
  return (
    <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2 text-[13px]">
      {rows.map(([k, v], i) => (
        <div key={i} className="contents">
          <dt className="text-muted">{k}</dt>
          <dd className="min-w-0 [overflow-wrap:anywhere]">{v}</dd>
        </div>
      ))}
    </dl>
  );
}
