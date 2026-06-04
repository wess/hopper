import type {
  FileFilter,
  FolderDialogOptions,
  FolderDialogResult,
  MessageDialogOptions,
  MessageDialogResult,
  OpenDialogOptions,
  OpenDialogResult,
  SaveDialogOptions,
  SaveDialogResult,
} from "butter/dialog";
import { dialog as butterDialog } from "butter/dialog";

export type {
  FileFilter,
  FolderDialogOptions,
  FolderDialogResult,
  MessageDialogOptions,
  MessageDialogResult,
  OpenDialogOptions,
  OpenDialogResult,
  SaveDialogOptions,
  SaveDialogResult,
};

export const openFile = async (opts: OpenDialogOptions = {}): Promise<string | undefined> => {
  const r = await butterDialog.open({ ...opts, multiple: false });
  return r.cancelled ? undefined : r.paths[0];
};

export const openFiles = async (opts: OpenDialogOptions = {}): Promise<readonly string[] | undefined> => {
  const r = await butterDialog.open({ ...opts, multiple: true });
  return r.cancelled ? undefined : r.paths;
};

export const saveFile = async (opts: SaveDialogOptions = {}): Promise<string | undefined> => {
  const r = await butterDialog.save(opts);
  return r.cancelled ? undefined : r.path;
};

export const openFolder = async (opts: FolderDialogOptions = {}): Promise<string | undefined> => {
  const r = await butterDialog.folder({ ...opts, multiple: false });
  return r.cancelled ? undefined : r.paths[0];
};

export const message = (opts: MessageDialogOptions): Promise<MessageDialogResult> => butterDialog.message(opts);

export const alert = (msg: string, title?: string): Promise<void> => butterDialog.alert(msg, title);

export const confirm = (msg: string, title?: string): Promise<boolean> => butterDialog.confirm(msg, title);
