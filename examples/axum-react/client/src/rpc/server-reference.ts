import { createORPCClient, isDefinedError, ORPCError } from "@orpc/client";
import { OpenAPILink } from "@orpc/openapi/fetch";
import { openapi } from "@orpc/openapi";
import { oc, type RouterContractClient } from "@orpc/contract";
import { asyncIteratorObject } from "@orpc/contract";
import { createTanstackQueryUtils } from "@orpc/tanstack-query";
import { z } from "zod";
export { consumeAsyncIterator, getEventMeta } from "@orpc/client";

const PlanetSchema = z.object({
  id: z.number(),
  name: z.string(),
  description: z.string().optional(),
});

// Contract router matching the Rust server's routes.
// All procedures use POST — oRPC's OpenAPILink sends input in the request body.
export const contract = {
  ping: oc.meta(openapi({ method: "POST", path: "/ping" }))
    .errors({
      RATE_LIMIT_EXCEEDED: {
        data: z.object({ retryAfter: z.number() })
      }
    })
    .output(z.string()),

  planet: {
    list: oc
      .meta(openapi({ method: "POST", path: "/planet/list" }))
      .output(z.array(PlanetSchema)),

    listPaginated: oc
      .meta(openapi({ method: "POST", path: "/planet/list-paginated" }))
      .input(z.object({ limit: z.number(), offset: z.number().optional() }))
      .output(
        z.object({
          items: z.array(PlanetSchema),
          nextPageParam: z.number().optional(),
        }),
      ),

    find: oc
      .meta(openapi({ method: "POST", path: "/planet/find" }))
      .errors({ 
        NOT_FOUND: { data: z.string() },
        STORE_ERROR: { data: z.string() }
      })
      .input(z.object({ id: z.number() }))
      .output(PlanetSchema),

    create: oc
      .meta(openapi({ method: "POST", path: "/planet/create" }))
      .errors({
        BAD_REQUEST: { data: z.string() },
        INTERNAL_ERROR: { data: z.string() },
        AUTHENTICATION_REQUIRED: { data: z.string() },
      })
      .input(z.object({ name: z.string(), description: z.string().optional() }))
      .output(PlanetSchema),
  },

  stream: oc
    .meta(openapi({ method: "POST", path: "/stream" }))
    .output(
      asyncIteratorObject(z.object({ message: z.string(), count: z.number() })),
    ),

  streamAsync: oc
    .meta(openapi({ method: "POST", path: "/stream-async" }))
    .output(
      asyncIteratorObject(z.object({ message: z.string(), count: z.number() })),
    ),
} as const;

const link = new OpenAPILink(contract, {
  origin: "http://localhost:3001",
  url: "/rpc",
  fetch(url, init, options, path) {
    console.log({ url, init, options, path });
    return globalThis.fetch(url, {
      ...init,
      credentials: "include",
    });
  },
});

export const client: RouterContractClient<typeof contract> =
  createORPCClient(link);

export const orpc = createTanstackQueryUtils(client);

export { isDefinedError, ORPCError };
