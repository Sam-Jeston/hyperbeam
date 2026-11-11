import { forwardRef, type InputHTMLAttributes, type ReactNode, type TextareaHTMLAttributes, useId } from "react";
import { cn } from "../../lib/utils";

const control =
  "w-full rounded-md border border-border-strong bg-background/60 px-3 text-[13.5px] text-foreground shadow-[inset_0_1px_0_hsl(var(--shadow-color)/0.04)] transition-[border-color,box-shadow] " +
  "hover:border-faint focus-visible:outline-none focus-visible:border-primary focus-visible:ring-3 focus-visible:ring-primary/20 " +
  "disabled:cursor-not-allowed disabled:opacity-60 aria-invalid:border-fail";

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement> & { mono?: boolean }>(
  ({ className, mono, type = "text", ...props }, ref) => (
    <input ref={ref} type={type} className={cn(control, "h-8.5", mono && "font-mono text-[12.5px]", className)} {...props} />
  ),
);
Input.displayName = "Input";

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement> & { mono?: boolean }>(
  ({ className, mono, ...props }, ref) => (
    <textarea
      ref={ref}
      className={cn(control, "min-h-20 resize-y py-2 leading-relaxed", mono && "font-mono text-[12.5px] leading-[1.6]", className)}
      {...props}
    />
  ),
);
Textarea.displayName = "Textarea";

export function Label({ className, ...props }: React.LabelHTMLAttributes<HTMLLabelElement>) {
  return <label className={cn("text-[13px] font-medium text-foreground", className)} {...props} />;
}

/**
 * A labelled form row. `children` receives the id to put on the control, so the
 * label, hint and error are all wired to it.
 */
export function Field(props: {
  label: ReactNode;
  hint?: ReactNode;
  optional?: boolean;
  className?: string;
  children: (id: string) => ReactNode;
}) {
  const id = useId();
  return (
    <div className={cn("grid content-start gap-1.5", props.className)}>
      <Label htmlFor={id} className="flex items-baseline gap-1.5">
        {props.label}
        {props.optional && <span className="text-xs font-normal text-faint">Optional</span>}
      </Label>
      {props.children(id)}
      {props.hint && <p className="text-[12.5px] leading-snug text-muted">{props.hint}</p>}
    </div>
  );
}

export function Kbd({ className, ...props }: React.HTMLAttributes<HTMLElement>) {
  return (
    <kbd
      className={cn(
        "inline-flex h-5 min-w-5 items-center justify-center rounded-[5px] border border-border-strong bg-surface-2 px-1 font-sans text-[11px] font-medium text-muted",
        className,
      )}
      {...props}
    />
  );
}
