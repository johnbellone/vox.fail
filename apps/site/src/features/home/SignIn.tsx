// Signing in or up: with Steam or Discord when the directory offers them, or a name and
// password.

import { Alert, Button, Divider, PasswordInput, SegmentedControl, Stack, TextInput } from "@mantine/core";
import { useForm } from "@mantine/form";
import { useState } from "react";
import { messageOf, post } from "../../api/client";
import type { Site } from "../../api/types";

export const SITES: Record<string, string> = { steam: "Steam", discord: "Discord" };

// what /auth/<site> sends back when it goes wrong
const FAILED: Record<string, string> = {
  steam: "Steam didn't confirm who you are. Try again.",
  discord: "Discord didn't confirm who you are. Try again.",
  cancelled: "Sign-in was cancelled.",
  expired: "That sign-in took too long or started in another tab. Try again.",
  busy: "Too many tries. Wait a few minutes, then try again.",
};

/** Back from a site that didn't let you in: what to say (and tidy the address bar). */
function siteError(): string | undefined {
  const why = new URLSearchParams(location.search).get("error");
  if (!why) return undefined;
  history.replaceState(null, "", "/");
  return FAILED[why] ?? "That didn't work. Try again.";
}

export function SignIn({ offered, onDone }: { offered: Site[]; onDone: () => void }) {
  const [mode, setMode] = useState<"in" | "up">("in");
  const [error, setError] = useState<string>();
  const [fromSite] = useState(siteError);
  const [busy, setBusy] = useState(false);
  const form = useForm({
    initialValues: { name: "", password: "" },
    validate: {
      name: (v) => (mode === "up" && !/^[\w .-]{3,20}$/.test(v.trim()) ? "3 to 20 letters, digits, spaces, _ - or ." : null),
      password: (v) => (mode === "up" && v.length < 8 ? "At least 8 characters." : null),
    },
  });
  const sites = offered.filter((s) => SITES[s]);

  async function submit(v: { name: string; password: string }) {
    setBusy(true);
    setError(undefined);
    try {
      await post(mode === "in" ? "/api/signin" : "/api/signup", { name: v.name.trim(), password: v.password });
      onDone();
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Stack gap="md">
      {fromSite && (
        <Alert color="ember" role="alert">
          {fromSite}
        </Alert>
      )}
      {sites.length > 0 && (
        <>
          <Stack gap="xs">
            {sites.map((s) => (
              <Button key={s} component="a" href={`/auth/${s}`} variant="default" size="md" fullWidth>
                Sign in with {SITES[s]}
              </Button>
            ))}
          </Stack>
          <Divider label="or with a name and password" labelPosition="center" />
        </>
      )}
      <SegmentedControl
        fullWidth
        value={mode}
        onChange={(m) => {
          setMode(m as "in" | "up");
          setError(undefined);
          form.clearErrors();
        }}
        data={[
          { value: "in", label: "Sign in" },
          { value: "up", label: "New account" },
        ]}
      />
      <form onSubmit={form.onSubmit(submit)} noValidate>
        <Stack gap="sm">
          <TextInput
            label="Name"
            autoComplete="username"
            maxLength={20}
            required
            description={mode === "up" ? "It's what others see over your head." : undefined}
            autoFocus={!sites.length}
            {...form.getInputProps("name")}
          />
          <PasswordInput
            label="Password"
            autoComplete={mode === "in" ? "current-password" : "new-password"}
            maxLength={128}
            required
            {...form.getInputProps("password")}
          />
          <Button type="submit" size="md" loading={busy}>
            {mode === "in" ? "Sign in" : "Make my account"}
          </Button>
          {error && (
            <Alert color="ember" role="alert">
              {error}
            </Alert>
          )}
        </Stack>
      </form>
    </Stack>
  );
}
