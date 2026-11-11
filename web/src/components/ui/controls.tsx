// Checkbox, switch, radio cards and toggle chips.

import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import * as RadioGroupPrimitive from "@radix-ui/react-radio-group";
import * as SwitchPrimitive from "@radix-ui/react-switch";
import { Check, Minus } from "lucide-react";
import { type ReactNode, useId } from "react";
import { cn } from "../../lib/utils";

export function Checkbox(props: {
  id?: string;
  checked: boolean | "indeterminate";
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  className?: string;
  "aria-label"?: string;
}) {
  return (
    <CheckboxPrimitive.Root
      id={props.id}
      checked={props.checked}
      onCheckedChange={(c) => props.onCheckedChange(c === true)}
      disabled={props.disabled}
      aria-label={props["aria-label"]}
      className={cn(
        "peer grid size-4 shrink-0 place-items-center rounded-[5px] border border-border-strong bg-background/60 transition-colors",
        "hover:border-faint focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring",
        "data-[state=checked]:border-primary data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground",
        "data-[state=indeterminate]:border-primary data-[state=indeterminate]:bg-primary data-[state=indeterminate]:text-primary-foreground",
        "disabled:cursor-not-allowed disabled:opacity-45",
        props.className,
      )}
    >
      <CheckboxPrimitive.Indicator>
        {props.checked === "indeterminate" ? <Minus className="size-3" strokeWidth={3} /> : <Check className="size-3" strokeWidth={3} />}
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  );
}

/** A checkbox with its label and an optional line of explanation. */
export function CheckRow(props: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  label: ReactNode;
  description?: ReactNode;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="flex items-start gap-2.5">
      <Checkbox id={id} checked={props.checked} onCheckedChange={props.onCheckedChange} disabled={props.disabled} className="mt-0.5" />
      <label htmlFor={id} className={cn("grid cursor-pointer gap-0.5 text-[13.5px] leading-snug", props.disabled && "cursor-not-allowed opacity-60")}>
        {props.label}
        {props.description && <span className="text-[12.5px] text-muted">{props.description}</span>}
      </label>
    </div>
  );
}

export function Switch(props: {
  id?: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  "aria-label"?: string;
}) {
  return (
    <SwitchPrimitive.Root
      id={props.id}
      checked={props.checked}
      onCheckedChange={props.onCheckedChange}
      disabled={props.disabled}
      aria-label={props["aria-label"]}
      className={cn(
        "inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border border-transparent bg-border-strong p-0.5 transition-colors",
        "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring data-[state=checked]:bg-primary disabled:cursor-not-allowed disabled:opacity-50",
      )}
    >
      <SwitchPrimitive.Thumb className="block size-4 rounded-full bg-white shadow-soft transition-transform data-[state=checked]:translate-x-4" />
    </SwitchPrimitive.Root>
  );
}

export function SwitchRow(props: { checked: boolean; onCheckedChange: (c: boolean) => void; label: ReactNode; description?: ReactNode }) {
  const id = useId();
  return (
    <div className="flex items-center justify-between gap-4">
      <label htmlFor={id} className="grid cursor-pointer gap-0.5 text-[13.5px]">
        <span className="font-medium">{props.label}</span>
        {props.description && <span className="text-[12.5px] text-muted">{props.description}</span>}
      </label>
      <Switch id={id} checked={props.checked} onCheckedChange={props.onCheckedChange} />
    </div>
  );
}

/** Large, explained choices — for decisions people should read before making. */
export function RadioCards<T extends string>(props: {
  value: T;
  onValueChange: (value: T) => void;
  options: { value: T; title: ReactNode; description: ReactNode; icon?: ReactNode }[];
  className?: string;
  "aria-label"?: string;
}) {
  return (
    <RadioGroupPrimitive.Root
      value={props.value}
      onValueChange={(v) => props.onValueChange(v as T)}
      aria-label={props["aria-label"]}
      className={cn("grid gap-2.5 sm:grid-cols-2", props.className)}
    >
      {props.options.map((o) => (
        <RadioGroupPrimitive.Item
          key={o.value}
          value={o.value}
          className={cn(
            "group flex cursor-pointer gap-3 rounded-lg border border-border bg-surface p-3 text-left transition-[border-color,background-color,box-shadow]",
            "hover:border-border-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring",
            "data-[state=checked]:border-primary/70 data-[state=checked]:bg-accent/45 data-[state=checked]:shadow-[0_0_0_3px_color-mix(in_srgb,var(--primary)_14%,transparent)]",
          )}
        >
          <span className="mt-0.5 grid size-4 shrink-0 place-items-center rounded-full border border-border-strong transition-colors group-data-[state=checked]:border-primary">
            <RadioGroupPrimitive.Indicator className="size-2 rounded-full bg-primary" />
          </span>
          <span className="grid gap-0.5">
            <span className="flex items-center gap-1.5 text-[13.5px] font-semibold [&_svg]:size-4 [&_svg]:text-muted">{o.icon}{o.title}</span>
            <span className="text-[12.5px] leading-snug text-muted">{o.description}</span>
          </span>
        </RadioGroupPrimitive.Item>
      ))}
    </RadioGroupPrimitive.Root>
  );
}

/** A pill that toggles on and off, for picking several items from a short list. */
export function ToggleChip(props: { pressed: boolean; onPressedChange: (p: boolean) => void; children: ReactNode; title?: string; mono?: boolean }) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={props.pressed}
      title={props.title}
      onClick={() => props.onPressedChange(!props.pressed)}
      className={cn(
        "inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-[12.5px] font-medium transition-colors",
        "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring",
        props.mono && "font-mono text-[12px]",
        props.pressed
          ? "border-primary/60 bg-accent text-accent-foreground"
          : "border-border-strong bg-surface text-muted hover:border-faint hover:text-foreground",
      )}
    >
      {props.pressed && <Check className="size-3.5" strokeWidth={2.5} />}
      {props.children}
    </button>
  );
}
