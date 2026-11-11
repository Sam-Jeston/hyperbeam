import * as AlertPrimitive from "@radix-ui/react-alert-dialog";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { type ReactNode, useSyncExternalStore } from "react";
import { cn } from "../../lib/utils";
import { Button } from "./button";

const overlay = "fixed inset-0 z-50 bg-overlay backdrop-blur-[2px] animate-fade-in";
const panel =
  "relative w-full rounded-xl border border-border bg-elevated text-foreground shadow-pop animate-pop-in focus:outline-none";

/**
 * A modal dialog. Mount it when it should show; closing calls `onClose`.
 * `footer` sits in a fixed bar under a body that scrolls on its own.
 */
export function Modal(props: {
  title: ReactNode;
  description?: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  size?: "sm" | "md" | "lg";
  /** Stop a click outside from closing it, for forms people could lose work in. */
  modalOnly?: boolean;
}) {
  return (
    <DialogPrimitive.Root open onOpenChange={(open) => !open && props.onClose()}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className={overlay} />
        <div className="pointer-events-none fixed inset-0 z-50 grid place-items-start justify-items-center overflow-y-auto px-4 py-[6vh]">
          <DialogPrimitive.Content
            className={cn(
              panel,
              "pointer-events-auto flex max-h-[88vh] flex-col",
              props.size === "lg" ? "max-w-[760px]" : props.size === "sm" ? "max-w-[440px]" : "max-w-[560px]",
            )}
            onInteractOutside={props.modalOnly ? (e) => e.preventDefault() : undefined}
          >
            <header className="flex items-start justify-between gap-4 px-5 pt-4.5 pb-1">
              <div className="grid gap-1">
                <DialogPrimitive.Title className="font-display text-[17px] font-semibold tracking-[-0.01em]">{props.title}</DialogPrimitive.Title>
                {props.description ? (
                  <DialogPrimitive.Description className="text-[13px] text-muted">{props.description}</DialogPrimitive.Description>
                ) : (
                  <DialogPrimitive.Description className="sr-only">{typeof props.title === "string" ? props.title : "Dialog"}</DialogPrimitive.Description>
                )}
              </div>
              <DialogPrimitive.Close asChild>
                <Button variant="ghost" size="icon-sm" aria-label="Close" className="-mr-1.5 -mt-0.5"><X /></Button>
              </DialogPrimitive.Close>
            </header>
            <div className="grid min-h-0 gap-4 overflow-y-auto px-5 py-4">{props.children}</div>
            {props.footer && (
              <footer className="flex flex-wrap items-center justify-end gap-2 rounded-b-xl border-t border-border bg-surface/60 px-5 py-3">{props.footer}</footer>
            )}
          </DialogPrimitive.Content>
        </div>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

/** A slide-in panel from the left, used for navigation on small screens. */
export function Sheet(props: { open: boolean; onOpenChange: (open: boolean) => void; title: string; children: ReactNode }) {
  return (
    <DialogPrimitive.Root open={props.open} onOpenChange={props.onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className={overlay} />
        <DialogPrimitive.Content className="fixed inset-y-0 left-0 z-50 flex w-[min(300px,86vw)] flex-col border-r border-border bg-surface shadow-pop animate-fade-in focus:outline-none">
          <DialogPrimitive.Title className="sr-only">{props.title}</DialogPrimitive.Title>
          <DialogPrimitive.Description className="sr-only">{props.title}</DialogPrimitive.Description>
          {props.children}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

// ------------------------------------------------------------- confirmations

interface ConfirmRequest {
  title: string;
  description?: ReactNode;
  confirmLabel?: string;
  destructive?: boolean;
  resolve: (ok: boolean) => void;
}

let pending: ConfirmRequest | null = null;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((fn) => fn());

/**
 * Ask before doing something that cannot be undone.
 * `if (await confirm({ title: "Remove repository?" })) …`
 */
export function confirm(options: Omit<ConfirmRequest, "resolve">): Promise<boolean> {
  pending?.resolve(false);
  return new Promise((resolve) => {
    pending = { ...options, resolve };
    emit();
  });
}

/** Renders whatever `confirm()` is asking. Mount once, near the root. */
export function ConfirmHost() {
  const request = useSyncExternalStore(
    (fn) => (listeners.add(fn), () => listeners.delete(fn)),
    () => pending,
  );
  const settle = (ok: boolean) => {
    request?.resolve(ok);
    pending = null;
    emit();
  };
  return (
    <AlertPrimitive.Root open={!!request} onOpenChange={(open) => !open && settle(false)}>
      <AlertPrimitive.Portal>
        <AlertPrimitive.Overlay className={overlay} />
        <div className="pointer-events-none fixed inset-0 z-50 grid place-items-center px-4">
          <AlertPrimitive.Content className={cn(panel, "pointer-events-auto max-w-[420px] p-5")}>
            <AlertPrimitive.Title className="font-display text-[17px] font-semibold tracking-[-0.01em]">{request?.title}</AlertPrimitive.Title>
            <AlertPrimitive.Description asChild>
              <div className="mt-1.5 text-[13.5px] text-muted">{request?.description}</div>
            </AlertPrimitive.Description>
            <div className="mt-5 flex justify-end gap-2">
              <AlertPrimitive.Cancel asChild>
                <Button variant="secondary">Cancel</Button>
              </AlertPrimitive.Cancel>
              <AlertPrimitive.Action asChild>
                <Button
                  variant={request?.destructive ? "danger" : "primary"}
                  className={request?.destructive ? "border-fail bg-fail text-white hover:bg-fail/90" : undefined}
                  onClick={() => settle(true)}
                >
                  {request?.confirmLabel ?? "Confirm"}
                </Button>
              </AlertPrimitive.Action>
            </div>
          </AlertPrimitive.Content>
        </div>
      </AlertPrimitive.Portal>
    </AlertPrimitive.Root>
  );
}
