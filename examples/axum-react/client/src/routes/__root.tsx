import { Link, Outlet, createRootRoute } from "@tanstack/react-router";

import { TanStackDevtools } from "@tanstack/react-devtools";

import { useSession, signOut } from "#/lib/auth-client";
import "../styles.css";

export const Route = createRootRoute({
  component: RootComponent,
});

function RootComponent() {
  const { data: session, isPending } = useSession();

  return (
    <>
      <main className="max-w-7xl mx-auto py-6 sm:px-6 lg:px-8">
        <Outlet />
      </main>
    </>
  );
}
