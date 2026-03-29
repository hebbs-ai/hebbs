import WorkspacePage from "./client";

export function generateStaticParams() {
  return [{ path: [] }];
}

export default function Page() {
  return <WorkspacePage />;
}
