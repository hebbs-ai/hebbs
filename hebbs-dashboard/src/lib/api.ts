const BASE = typeof window !== "undefined" ? window.location.origin : "";

async function request<T>(path: string, opts: RequestInit = {}): Promise<T> {
  const token =
    typeof window !== "undefined" ? localStorage.getItem("hebbs_token") : null;

  const headers: Record<string, string> = {
    ...(opts.headers as Record<string, string>),
  };
  if (token) headers["Authorization"] = `Bearer ${token}`;
  // Only set Content-Type for JSON bodies, not FormData (browser sets multipart boundary)
  if (opts.body && typeof opts.body === "string")
    headers["Content-Type"] = "application/json";

  const fetchOpts: RequestInit = { ...opts, headers };
  // For FormData, remove Content-Type so browser auto-sets multipart/form-data with boundary
  if (opts.body instanceof FormData) {
    delete headers["Content-Type"];
  }

  const res = await fetch(`${BASE}${path}`, fetchOpts);

  if (res.status === 401) {
    if (typeof window !== "undefined" && !path.includes("/auth/login")) {
      localStorage.removeItem("hebbs_token");
      window.location.href = "/login/";
    }
    throw new Error("Unauthorized");
  }

  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || `HTTP ${res.status}`);
  }

  return res.json();
}

export const api = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body?: unknown) =>
    request<T>(path, {
      method: "POST",
      body: body ? JSON.stringify(body) : undefined,
    }),
  put: <T>(path: string, body?: unknown) =>
    request<T>(path, {
      method: "PUT",
      body: body ? JSON.stringify(body) : undefined,
    }),
  del: <T>(path: string) => request<T>(path, { method: "DELETE" }),
  upload: <T>(path: string, formData: FormData) =>
    request<T>(path, { method: "POST", body: formData }),
};
