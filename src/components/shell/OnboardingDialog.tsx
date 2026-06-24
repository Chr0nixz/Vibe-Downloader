import { useState } from "react";
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
import { cn } from "@/lib/utils";

const ONBOARDING_STORAGE_KEY = "vibe-onboarding-completed";
const TOTAL_STEPS = 3;

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

export function OnboardingDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const { t } = useTranslation();
  const [step, setStep] = useState(0);

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

  const isLastStep = step === TOTAL_STEPS - 1;

  const stepTitleKey = ["onboarding.step1Title", "onboarding.step2Title", "onboarding.step3Title"][step];
  const stepBodyKey = ["onboarding.step1Body", "onboarding.step2Body", "onboarding.step3Body"][step];

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
          <DialogDescription className="sr-only">{t("onboarding.title")}</DialogDescription>
        </DialogHeader>
        <DialogBody className="space-y-4">
          {/* Step indicator */}
          <div className="flex items-center justify-center gap-2" role="group" aria-label={t("onboarding.title")}>
            {Array.from({ length: TOTAL_STEPS }, (_, i) => (
              <span
                key={i}
                aria-hidden="true"
                className={cn(
                  "h-1.5 rounded-full transition-all duration-[var(--motion-ui)]",
                  i === step ? "w-6 bg-accent-primary" : "w-1.5 bg-border-subtle",
                )}
              />
            ))}
          </div>

          <div className="space-y-2">
            <h3 className="text-base font-semibold text-text-primary">{t(stepTitleKey)}</h3>
            <p className="text-sm leading-relaxed text-text-secondary">{t(stepBodyKey)}</p>
          </div>
        </DialogBody>
        <DialogFooter className="flex-row justify-between gap-2">
          <Button type="button" variant="ghost" onClick={handleClose}>
            {t("onboarding.skip")}
          </Button>
          <Button type="button" variant="default" onClick={handleNext}>
            {isLastStep ? t("onboarding.start") : t("onboarding.next")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
