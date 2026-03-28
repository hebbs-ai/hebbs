import { loadConfig } from "./config.js";

class ApiError extends Error {
  constructor(
    public status: number,
    message: string
  ) {
    super(message);
  }
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  formData?: FormData
): Promise<T> {
  const config = loadConfig();
  if (!config.endpoint) {
    console.error("Not logged in. Run: hb login --endpoint <url>");
    process.exit(1);
  }

  const headers: Record<string, string> = {};
  if (config.api_key) headers["Authorization"] = `Bearer ${config.api_key}`;
  if (body) headers["Content-Type"] = "application/json";

  const res = await fetch(`${config.endpoint}${path}`, {
    method,
    headers,
    body: formData || (body ? JSON.stringify(body) : undefined),
  });

  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new ApiError(
      res.status,
      (data as Record<string, string>).error || `HTTP ${res.status}`
    );
  }

  return res.json() as Promise<T>;
}

export const api = {
  get: <T>(path: string) => request<T>("GET", path),
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, body),
  put: <T>(path: string, body?: unknown) => request<T>("PUT", path, body),
  del: <T>(path: string) => request<T>("DELETE", path),
};
