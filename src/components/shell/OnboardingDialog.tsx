import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { getBrowserIntegrationStatus } from "@/lib/tauri";
import { cn } from "@/lib/utils";

const ONBOARDING_STORAGE_KEY = "vibe-onboarding-completed";
const TOTAL_STEPS = 5;

export function markOnboardingCompleted() {
  try {
    localStorage.setItem(ONBOARDING_STORAGE_KEY, "1");
  } catch {
    // localStorage unavailable; onboarding will re-show on next launch
  }
}

export function shouldShowOnboarding(): boolean {
  try {
    return localStorage.getItem(ONBOARDING_STORAGE_KEY) !== "1";
  } catch {
    return false;
  }
}

export function OnboardingDialog({
  open,
  onOpenChange,
  onOpenSettings,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  onOpenSettings?: () => void;
}) {
  const { t } = useTranslation();
  const [step, setStep] = useState(0);
  const [extensionInstalled, setExtensionInstalled] = useState(false);

  // Check browser extension installation status when reaching step 2 (index 2 = extension step)
  useEffect(() => {
    if (!open || step !== 2) return;
    let cancelled = false;
    void getBrowserIntegrationStatus()
      .then((status) => {
        if (cancelled) return;
        const installed = status.browsers.some((b) => b.manifestInstalled);
        setExtensionInstalled(installed);
      })
      .catch(() => {
        // Non-Tauri runtime or status unavailable; leave as false
      });
    return () => {
      cancelled = true;
    };
  }, [open, step]);

  const handleClose = () => {
    markOnboardingCompleted();
    onOpenChange(false);
  };

  const handleNext = () => {
    if (step < TOTAL_STEPS - 1) {
      setStep(step + 1);
    } else {
      handleClose();
    }
  };

  const handleBack = () => {
    if (step > 0) setStep(step - 1);
  };

  const isLastStep = step === TOTAL_STEPS - 1;
  const isFirstStep = step === 0;

  const stepTitleKey = [
    "onboarding.step1Title",
    "onboarding.step2Title",
    "onboarding.step3Title",
    "onboarding.step4Title",
    "onboarding.step5Title",
  ][step];
  const stepBodyKey = [
    "onboarding.step1Body",
    "onboarding.step2Body",
    "onboarding.step3Body",
    "onboarding.step4Body",
    "onboarding.step5Body",
  ][step];

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v) handleClose();
        else onOpenChange(v);
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("onboarding.title")}</DialogTitle>
          <DialogDescription className="sr-only">
            {t("onboarding.stepIndicator", { current: step + 1, total: TOTAL_STEPS })}
          </DialogDescription>
        </DialogHeader>
        <DialogBody className="space-y-4">
          {/* Step indicator */}
          <ol
            className="m-0 flex list-none items-center justify-center gap-2 p-0"
            aria-label={t("onboarding.stepIndicator", { current: step + 1, total: TOTAL_STEPS })}
          >
            {Array.from({ length: TOTAL_STEPS }, (_, i) => (
              <li
                key={i}
                aria-hidden={i !== step ? "true" : undefined}
                aria-current={i === step ? "step" : undefined}
                className={cn(
                  "h-1.5 rounded-full transition-all duration-[var(--motion-ui)]",
                  i === step ? "w-6 bg-accent-primary" : "w-1.5 bg-border-subtle",
                )}
              />
            ))}
          </ol>

          <div className="space-y-2">
            <h3 className="text-base font-semibold text-text-primary">{t(stepTitleKey)}</h3>
            <p className="text-sm leading-relaxed text-text-secondary">{t(stepBodyKey)}</p>
          </div>

          {/* Step 2 (index 2): browser extension install action */}
          {step === 2 ? (
            <div className="pt-1">
              {extensionInstalled ? (
                <p className="text-sm font-medium text-status-success">{t("onboarding.extensionInstalled")}</p>
              ) : (
                <Button
                  type="button"
                  variant="default"
                  className="w-full"
                  onClick={() => {
                    onOpenSettings?.();
                    handleClose();
                  }}
                >
                  {t("onboarding.installExtension")}
                </Button>
              )}
            </div>
          ) : null}
        </DialogBody>
        <DialogFooter className="flex-row justify-between gap-2">
          <Button type="button" variant="ghost" onClick={handleClose}>
            {t("onboarding.skip")}
          </Button>
          <div className="flex gap-2">
            {!isFirstStep ? (
              <Button type="button" variant="ghost" onClick={handleBack}>
                {t("onboarding.back")}
              </Button>
            ) : null}
            <Button type="button" variant="default" onClick={handleNext}>
              {isLastStep ? t("onboarding.start") : t("onboarding.next")}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
