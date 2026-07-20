import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SftpKnownHostsEditor } from "@/components/settings/SftpKnownHostsEditor";

const listSftpKnownHosts = vi.fn();
const forgetSftpKnownHost = vi.fn();

vi.mock("@/lib/tauri", () => ({
  listSftpKnownHosts: (...args: unknown[]) => listSftpKnownHosts(...args),
  forgetSftpKnownHost: (...args: unknown[]) => forgetSftpKnownHost(...args),
}));

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, params?: Record<string, string>) => {
        if (params?.host) return `${key}:${params.host}`;
        return key;
      },
      i18n: { language: "en" },
    }),
  };
});

const sampleHost = {
  host: "sftp.example.com",
  port: 22,
  algorithm: "ssh-ed25519",
  fingerprintSha256: "AA:BB:CC",
  firstSeenAt: "2026-01-01T00:00:00Z",
  lastSeenAt: "2026-01-02T00:00:00Z",
};

describe("SftpKnownHostsEditor ARC-15", () => {
  beforeEach(() => {
    listSftpKnownHosts.mockReset();
    forgetSftpKnownHost.mockReset();
    listSftpKnownHosts.mockResolvedValue([sampleHost]);
    forgetSftpKnownHost.mockResolvedValue(true);
  });

  it("lists known hosts and confirms forget before calling the API", async () => {
    const user = userEvent.setup();
    render(<SftpKnownHostsEditor />);

    await waitFor(() => {
      expect(screen.getByText("sftp.example.com:22")).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: "settings.sftpKnownHostsForget" }));
    expect(screen.getByText(/settings.sftpKnownHostsForgetTitle/)).toBeInTheDocument();
    expect(forgetSftpKnownHost).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "settings.sftpKnownHostsForgetConfirm" }));
    await waitFor(() => {
      expect(forgetSftpKnownHost).toHaveBeenCalledWith("sftp.example.com", 22);
    });
  });

  it("cancel closes the dialog without forgetting", async () => {
    const user = userEvent.setup();
    render(<SftpKnownHostsEditor />);

    await waitFor(() => {
      expect(screen.getByText("sftp.example.com:22")).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: "settings.sftpKnownHostsForget" }));
    await user.click(screen.getByRole("button", { name: "recoveryDialog.cancel" }));
    expect(forgetSftpKnownHost).not.toHaveBeenCalled();
  });
});
