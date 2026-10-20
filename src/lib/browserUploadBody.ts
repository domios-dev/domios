import { MAX_TEMP_FILE_DECODED_BYTES } from "../../generated/config/attachmentLimits";

export async function browserUploadBody(file: Blob, filename: string): Promise<FormData> {
  if (file.size > MAX_TEMP_FILE_DECODED_BYTES) throw Object.assign(new Error("Attachment exceeds the upload size limit"), { status: 413 });
  const form = new FormData();
  form.append("file", file, filename);
  return form;
}
