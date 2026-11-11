// Dropdown menus, tooltips and tabs.

import * as DropdownPrimitive from "@radix-ui/react-dropdown-menu";
import * as TabsPrimitive from "@radix-ui/react-tabs";
import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import { Check } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "../../lib/utils";

// ---------------------------------------------------------------- dropdowns

export const Menu = DropdownPrimitive.Root;
export const MenuTrigger = DropdownPrimitive.Trigger;

export function MenuContent(props: { children: ReactNode; align?: "start" | "end" | "center"; side?: "top" | "bottom" | "left" | "right"; className?: string }) {
  return (
    <DropdownPrimitive.Portal>
      <DropdownPrimitive.Content
        align={props.align ?? "start"}
        side={props.side}
        sideOffset={6}
        className={cn("z-50 min-w-48 rounded-lg border border-border bg-elevated p-1 text-foreground shadow-pop animate-pop-in", props.className)}
      >
        {props.children}
      </DropdownPrimitive.Content>
    </DropdownPrimitive.Portal>
  );
}

const item =
  "relative flex cursor-pointer select-none items-center gap-2 rounded-md px-2 py-1.5 text-[13.5px] outline-none data-[highlighted]:bg-surface-2 data-[disabled]:pointer-events-none data-[disabled]:opacity-50 [&_svg]:size-4 [&_svg]:text-muted";

export function MenuItem(props: { children: ReactNode; onSelect?: () => void; destructive?: boolean; disabled?: boolean }) {
  return (
    <DropdownPrimitive.Item onSelect={props.onSelect} disabled={props.disabled} className={cn(item, props.destructive && "text-fail [&_svg]:text-fail")}>
      {props.children}
    </DropdownPrimitive.Item>
  );
}

export function MenuRadio(props: { value: string; onValueChange: (v: string) => void; options: { value: string; label: ReactNode; icon?: ReactNode }[] }) {
  return (
    <DropdownPrimitive.RadioGroup value={props.value} onValueChange={props.onValueChange}>
      {props.options.map((o) => (
        <DropdownPrimitive.RadioItem key={o.value} value={o.value} className={cn(item, "pr-8")} onSelect={(e) => e.preventDefault()}>
          {o.icon}
          {o.label}
          <DropdownPrimitive.ItemIndicator className="absolute right-2">
            <Check className="text-primary!" />
          </DropdownPrimitive.ItemIndicator>
        </DropdownPrimitive.RadioItem>
      ))}
    </DropdownPrimitive.RadioGroup>
  );
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return <DropdownPrimitive.Label className="px-2 pt-1.5 pb-1 text-[11px] font-semibold uppercase tracking-[0.06em] text-faint">{children}</DropdownPrimitive.Label>;
}

export function MenuSeparator() {
  return <DropdownPrimitive.Separator className="-mx-1 my-1 h-px bg-border" />;
}

// ----------------------------------------------------------------- tooltips

export const TooltipProvider = TooltipPrimitive.Provider;

export function Tooltip(props: { content: ReactNode; children: ReactNode; side?: "top" | "bottom" | "left" | "right" }) {
  if (!props.content) return <>{props.children}</>;
  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger asChild>{props.children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          side={props.side ?? "top"}
          sideOffset={6}
          className="z-50 max-w-72 rounded-md border border-border bg-elevated px-2.5 py-1.5 text-[12.5px] leading-snug text-foreground shadow-pop animate-fade-in"
        >
          {props.content}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}

// --------------------------------------------------------------------- tabs

const tabList = "flex items-center gap-1 overflow-x-auto border-b border-border";
const tab =
  "relative -mb-px inline-flex h-9 shrink-0 items-center gap-1.5 border-b-2 border-transparent px-2.5 text-[13.5px] font-medium text-muted transition-colors hover:text-foreground " +
  "focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring";
const tabActive = "border-primary text-foreground";

export function TabCount({ n }: { n: number }) {
  return <span className="rounded-full bg-surface-2 px-1.5 text-[11px] font-semibold tabular-nums text-muted">{n}</span>;
}

/** Tabs that switch content in place. */
export function Tabs<T extends string>(props: { value: T; onValueChange: (v: T) => void; tabs: { value: T; label: ReactNode }[]; className?: string }) {
  return (
    <TabsPrimitive.Root value={props.value} onValueChange={(v) => props.onValueChange(v as T)}>
      <TabsPrimitive.List className={cn(tabList, props.className)}>
        {props.tabs.map((t) => (
          <TabsPrimitive.Trigger key={t.value} value={t.value} className={cn(tab, "data-[state=active]:border-primary data-[state=active]:text-foreground")}>
            {t.label}
          </TabsPrimitive.Trigger>
        ))}
      </TabsPrimitive.List>
    </TabsPrimitive.Root>
  );
}

/** Tabs that are links, for sections with their own URL. */
export function LinkTabs(props: { active: string; tabs: { value: string; label: ReactNode; href: string }[]; onNavigate: (href: string) => void; className?: string }) {
  return (
    <nav className={cn(tabList, props.className)}>
      {props.tabs.map((t) => (
        <a
          key={t.value}
          href={t.href}
          aria-current={props.active === t.value ? "page" : undefined}
          className={cn(tab, props.active === t.value && tabActive)}
          onClick={(e) => {
            if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
            e.preventDefault();
            props.onNavigate(t.href);
          }}
        >
          {t.label}
        </a>
      ))}
    </nav>
  );
}
