export const MAIL_REFRESH_MS = 30_000;
export type Folder = "inbox" | "spam" | "trash" | "sent";
export type MailAddress = {
  address: string;
  mailbox: string;
  kind: string;
  name: string;
  unlocked: boolean;
  sending_unlocked?: boolean;
  can_send?: boolean;
  unread?: number | null;
  unread_limited?: boolean;
};
export type MailMessage = {
  uid: number;
  subject: string;
  from: string;
  to: string;
  date: string;
  read: boolean;
  size: number;
  reply_to?: string;
};
export type MessageBody = {
  message: MailMessage;
  text: string;
  html: string;
  attachments: { part: number; name: string; size: number }[];
};
export type Reply = { uid: number; validity: number; folder: Folder };
