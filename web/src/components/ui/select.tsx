import * as SelectPrimitive from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "../../lib/utils";

export interface SelectOption {
  value: string;
  label: ReactNode;
  /** Shown under the label in the open list only. */
  description?: ReactNode;
  disabled?: boolean;
}

// Radix reserves the empty string; we use it to mean "the default".
const EMPTY = "__default__";

/** A styled single-choice dropdown. `""` is a valid value, typically "use the default". */
export function Select(props: {
  id?: string;
  value: string;
  onValueChange: (value: string) => void;
  options: SelectOption[];
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  size?: "sm" | "md";
  "aria-label"?: string;
}) {
  return (
    <SelectPrimitive.Root
      value={props.value === "" ? EMPTY : props.value}
      onValueChange={(v) => props.onValueChange(v === EMPTY ? "" : v)}
      disabled={props.disabled}
    >
      <SelectPrimitive.Trigger
        id={props.id}
        aria-label={props["aria-label"]}
        className={cn(
          "group inline-flex w-full items-center justify-between gap-2 rounded-md border border-border-strong bg-background/60 px-3 text-left text-[13.5px] text-foreground transition-[border-color,box-shadow]",
          "hover:border-faint focus-visible:outline-none focus-visible:border-primary focus-visible:ring-3 focus-visible:ring-primary/20 disabled:opacity-60 data-[placeholder]:text-faint",
          props.size === "sm" ? "h-7 text-[12.5px] px-2.5" : "h-8.5",
          props.className,
        )}
      >
        <span className="truncate"><SelectPrimitive.Value placeholder={props.placeholder} /></span>
        <SelectPrimitive.Icon asChild>
          <ChevronDown className="size-4 shrink-0 text-faint transition-transform group-data-[state=open]:rotate-180" />
        </SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Content
          position="popper"
          sideOffset={4}
          className="z-50 max-h-[min(360px,var(--radix-select-content-available-height))] min-w-[var(--radix-select-trigger-width)] overflow-hidden rounded-lg border border-border bg-elevated text-foreground shadow-pop animate-pop-in"
        >
          <SelectPrimitive.Viewport className="p-1">
            {props.options.map((o) => (
              <SelectPrimitive.Item
                key={o.value || EMPTY}
                value={o.value === "" ? EMPTY : o.value}
                disabled={o.disabled}
                className="relative flex cursor-pointer select-none flex-col rounded-md py-1.5 pl-2.5 pr-8 text-[13.5px] outline-none data-[highlighted]:bg-surface-2 data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50"
              >
                <SelectPrimitive.ItemText>{o.label}</SelectPrimitive.ItemText>
                {o.description && <span className="text-xs text-muted">{o.description}</span>}
                <SelectPrimitive.ItemIndicator className="absolute right-2.5 top-2">
                  <Check className="size-4 text-primary" />
                </SelectPrimitive.ItemIndicator>
              </SelectPrimitive.Item>
            ))}
          </SelectPrimitive.Viewport>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}
