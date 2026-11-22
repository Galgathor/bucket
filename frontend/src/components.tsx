import type { ReactNode } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import * as Popover from "@radix-ui/react-popover";
import { Info, X, LoaderCircle } from "lucide-react";
import type { Policy } from "./api";
export function InfoButton({
  children,
  label = "More information",
}: {
  children: ReactNode;
  label?: string;
}) {
  return (
    <Popover.Root>
      <Popover.Trigger className="icon-button info" aria-label={label}>
        <Info size={15} />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className="popover"
          sideOffset={8}
          collisionPadding={16}
        >
          {children}
          <Popover.Arrow className="popover-arrow" />
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
export function Modal({
  title,
  description,
  children,
  onClose,
}: {
  title: string;
  description?: string;
  children: ReactNode;
  onClose: () => void;
}) {
  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="overlay" />
        <Dialog.Content
          className="modal"
          onInteractOutside={(e) => e.preventDefault()}
        >
          <div className="modal-heading">
            <div>
              <Dialog.Title>{title}</Dialog.Title>
              {description ? (
                <Dialog.Description>{description}</Dialog.Description>
              ) : (
                <Dialog.Description className="sr-only">
                  {title}
                </Dialog.Description>
              )}
            </div>
            <Dialog.Close className="icon-button" aria-label="Close">
              <X size={20} />
            </Dialog.Close>
          </div>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
export function Busy() {
  return <LoaderCircle size={18} className="spin" aria-label="Loading" />;
}
export function ErrorMessage({ message }: { message: string }) {
  return message ? (
    <div role="alert" className="error-message">
      {message}
    </div>
  ) : null;
}
export function PolicyFields({
  value,
  onChange,
  disabled = false,
}: {
  value: Policy;
  onChange: (p: Policy) => void;
  disabled?: boolean;
}) {
  const fields: [keyof Policy, string][] = [
    ["max_mailboxes", "Mailboxes per person"],
    ["max_aliases", "Aliases per person"],
    ["default_storage", "Default storage (MB)"],
    ["max_storage", "Maximum per mailbox (MB)"],
    ["total_storage", "Total storage per person (MB)"],
  ];
  return (
    <fieldset disabled={disabled} className="policy-fields">
      <div className="check-row">
        <label>
          <input
            type="checkbox"
            checked={value.mailboxes}
            onChange={(e) =>
              onChange({ ...value, mailboxes: e.target.checked })
            }
          />{" "}
          Allow mailboxes
        </label>
        <InfoButton>A mailbox has its own inbox and password.</InfoButton>
      </div>
      <div className="check-row">
        <label>
          <input
            type="checkbox"
            checked={value.aliases}
            onChange={(e) => onChange({ ...value, aliases: e.target.checked })}
          />{" "}
          Allow aliases
        </label>
        <InfoButton>
          An alias forwards messages to an existing mailbox.
        </InfoButton>
      </div>
      <div className="form-grid">
        {fields.map(([key, label]) => (
          <label key={key}>
            {label}
            <input
              type="number"
              required
              min={key.includes("storage") ? 128 : 0}
              max={
                key === "max_aliases"
                  ? 10000
                  : key === "max_mailboxes"
                    ? 1000
                    : key === "total_storage"
                      ? 104857600
                      : 1048576
              }
              value={value[key] as number}
              onChange={(e) =>
                onChange({ ...value, [key]: e.target.valueAsNumber || 0 })
              }
            />
          </label>
        ))}
      </div>
    </fieldset>
  );
}
