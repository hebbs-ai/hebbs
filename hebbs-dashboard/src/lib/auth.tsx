"use client";

import {
  createContext,
  useContext,
  useState,
  useEffect,
  type ReactNode,
} from "react";
import { api } from "./api";

interface Account {
  id: number;
  email: string;
  role: "admin" | "developer";
}

interface AuthCtx {
  account: Account | null;
  loading: boolean;
  login: (email: string, password: string) => Promise<void>;
  logout: () => void;
}

const Ctx = createContext<AuthCtx>({
  account: null,
  loading: true,
  login: async () => {},
  logout: () => {},
});

export function AuthProvider({ children }: { children: ReactNode }) {
  const [account, setAccount] = useState<Account | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const token = localStorage.getItem("hebbs_token");
    if (!token) {
      setLoading(false);
      return;
    }
    api
      .get<{ account: Account }>("/v1/auth/me")
      .then((d) => setAccount(d.account))
      .catch(() => localStorage.removeItem("hebbs_token"))
      .finally(() => setLoading(false));
  }, []);

  const login = async (email: string, password: string) => {
    const res = await api.post<{ token: string; account: Account }>(
      "/v1/auth/login",
      { email, password }
    );
    localStorage.setItem("hebbs_token", res.token);
    setAccount(res.account);
  };

  const logout = () => {
    api.post("/v1/auth/logout").catch(() => {});
    localStorage.removeItem("hebbs_token");
    setAccount(null);
    window.location.href = "/login/";
  };

  return (
    <Ctx.Provider value={{ account, loading, login, logout }}>
      {children}
    </Ctx.Provider>
  );
}

export const useAuth = () => useContext(Ctx);
