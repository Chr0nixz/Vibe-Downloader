import * as DialogPrimitive from "@radix-ui/react-dialog";
import { motion, useReducedMotion } from "motion/react";
import * as React from "react";

import { acquireModalFocus, isModalFocusActive } from "@/lib/modal-focus";
import { cn } from "@/lib/utils";

const Dialog = DialogPrimitive.Root;
const DialogTrigger = DialogPrimitive.Trigger;
const DialogPortal = DialogPrimitive.Portal;
const DialogClose = DialogPrimitive.Close;

const DialogOverlay = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>
>(({ className, ...props }, ref) => {
  const reduceMotion = useReducedMotion();

  return (
    <DialogPrimitive.Overlay asChild {...props}>
      <motion.div
        ref={ref}
        className={cn("fixed inset-0 z-50 bg-surface-scrim", className)}
        initial={reduceMotion ? false : { opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.18, ease: "easeOut" }}
      />
    </DialogPrimitive.Overlay>
  );
});
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

/**
 * Claims modal focus for as long as it is mounted. It must render inside
 * `DialogPrimitive.Content`, which Radix mounts only while the dialog is open:
 * the `DialogContent` wrapper itself stays mounted under `<Dialog open={false}>`,
 * so claiming there held the claim for the whole session whenever a dialog was
 * kept mounted (NewDownloadDialog is) and deferred every non-error toast,
 * Undo included, forever.
 */
function ModalFocusClaim({ returnFocusRef }: { returnFocusRef: React.MutableRefObject<HTMLElement | null> }) {
  // Layout effects run child-first, so this reads the focus owner before
  // Radix's FocusScope (an ancestor) moves focus into the dialog.
  React.useLayoutEffect(() => {
    const active = document.activeElement;
    returnFocusRef.current = active instanceof HTMLElement && active !== document.body ? active : null;
  }, [returnFocusRef]);
  React.useEffect(() => acquireModalFocus(), []);
  return null;
}

const DialogContent = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content>
>(({ className, children, onCloseAutoFocus, ...props }, ref) => {
  const reduceMotion = useReducedMotion();
  const returnFocusRef = React.useRef<HTMLElement | null>(null);

  return (
    <DialogPortal>
      <DialogOverlay />
      <DialogPrimitive.Content
        asChild
        {...props}
        onCloseAutoFocus={(event) => {
          onCloseAutoFocus?.(event);
          if (event.defaultPrevented) return;
          // Radix returns focus to a DialogTrigger, but most dialogs here open
          // from shortcuts, the palette, or row menus and have no trigger, so
          // focus fell to <body> and arrow keys stopped reaching the task list.
          // When a palette command opens another dialog, that dialog owns
          // focus now; only hand focus back into the page or a parent dialog.
          const target = returnFocusRef.current;
          returnFocusRef.current = null;
          const handingToPage = !isModalFocusActive() || target?.closest('[role="dialog"], [role="alertdialog"]');
          if (target?.isConnected && handingToPage) {
            event.preventDefault();
            target.focus({ preventScroll: true });
          }
        }}
      >
        <motion.div
          ref={ref}
          className={cn(
            "fixed left-1/2 z-50 flex w-[calc(100%-1.5rem)] max-w-lg -translate-x-1/2 flex-col overflow-hidden rounded-lg bg-surface-popover shadow-md ring-1 ring-border-subtle focus:outline-none",
            "top-1/2 max-h-[calc(100dvh-1.5rem)] -translate-y-1/2",
            "md:top-[16%] md:max-h-[min(40rem,calc(100dvh-2rem))] md:translate-y-0",
            className,
          )}
          initial={reduceMotion ? false : { opacity: 0, y: 10, scale: 0.98 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={reduceMotion ? { opacity: 0 } : { opacity: 0, y: 8, scale: 0.98 }}
          transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
        >
          <ModalFocusClaim returnFocusRef={returnFocusRef} />
          {children}
        </motion.div>
      </DialogPrimitive.Content>
    </DialogPortal>
  );
});
DialogContent.displayName = DialogPrimitive.Content.displayName;

const DialogHeader = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("shrink-0 border-b border-border-subtle px-4 py-3", className)} {...props} />
);

const DialogBody = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-4", className)} {...props} />
);

const DialogFooter = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      "flex shrink-0 flex-col-reverse gap-2 border-t border-border-subtle px-4 py-3 sm:flex-row sm:justify-end",
      className,
    )}
    {...props}
  />
);

const DialogTitle = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  // 16px semibold over 14px body copy: at 14px medium the title was one weight
  // step from the text under it and dialogs read as one flat block.
  <DialogPrimitive.Title ref={ref} className={cn("text-base font-semibold text-text-primary", className)} {...props} />
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;

const DialogDescription = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description ref={ref} className={cn("text-sm text-text-muted", className)} {...props} />
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;

export {
  Dialog,
  DialogBody,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogOverlay,
  DialogPortal,
  DialogTitle,
  DialogTrigger,
};
