import { Button, Group, TextInput } from "@mantine/core";
import { useState } from "react";

/** One text box and a button: ask a friend, name a group or party. */
export function NameForm({
  label,
  placeholder,
  button,
  maxLength = 20,
  onSubmit,
  busy,
}: {
  label: string;
  placeholder: string;
  button: string;
  maxLength?: number;
  onSubmit: (name: string) => Promise<boolean>;
  busy?: boolean;
}) {
  const [value, setValue] = useState("");
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        const name = value.trim();
        if (name && (await onSubmit(name))) setValue("");
      }}
    >
      <Group gap="xs" align="flex-end" wrap="nowrap">
        <TextInput
          label={label}
          placeholder={placeholder}
          value={value}
          onChange={(e) => setValue(e.currentTarget.value)}
          maxLength={maxLength}
          autoComplete="off"
          required
          style={{ flex: 1 }}
        />
        <Button type="submit" loading={busy}>
          {button}
        </Button>
      </Group>
    </form>
  );
}
