export async function browserUploadBody(file: Blob, filename: string): Promise<FormData> {
  const bytes = await file.arrayBuffer();
  if (bytes.byteLength !== file.size) throw new TypeError("Attachment bytes are unavailable; retry the upload");
  const form = new FormData();
  form.append("file", new Blob([bytes], { type: file.type }), filename);
  return form;
}
