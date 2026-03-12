"use client";

import * as React from "react";

import { cn } from "@/lib/utils";

type TabsContextValue = {
  value: string;
  onValueChange?: (value: string) => void;
};

const TabsContext = React.createContext<TabsContextValue | null>(null);

function useTabsContext() {
  const context = React.useContext(TabsContext);

  if (!context) {
    throw new Error("Tabs components must be used within <Tabs />");
  }

  return context;
}

function Tabs({
  value,
  onValueChange,
  className,
  children,
}: React.ComponentProps<"div"> & TabsContextValue) {
  return (
    <TabsContext.Provider value={{ value, onValueChange }}>
      <div className={cn("flex flex-col gap-4", className)}>{children}</div>
    </TabsContext.Provider>
  );
}

function TabsList({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      role="tablist"
      className={cn(
        "inline-flex flex-wrap items-center gap-1 rounded-xl border border-[#d6dde8] bg-[#e3e8ef] p-1.5 shadow-none",
        className
      )}
      {...props}
    />
  );
}

function TabsTrigger({
  value,
  className,
  children,
  ...props
}: React.ComponentProps<"button"> & { value: string }) {
  const context = useTabsContext();
  const isActive = context.value === value;

  return (
    <button
      role="tab"
      type="button"
      aria-selected={isActive}
      data-state={isActive ? "active" : "inactive"}
      className={cn(
        "inline-flex min-w-fit items-center justify-center rounded-[10px] px-4 py-2.5 text-sm font-medium transition-colors",
        "data-[state=active]:bg-white data-[state=active]:text-[#17233c] data-[state=active]:shadow-[0_1px_2px_rgba(15,23,42,0.12)]",
        "data-[state=inactive]:text-[#64748b] hover:bg-[#eef2f7] hover:text-[#17233c]",
        className
      )}
      onClick={() => context.onValueChange?.(value)}
      {...props}
    >
      {children}
    </button>
  );
}

function TabsContent({
  value,
  className,
  children,
  ...props
}: React.ComponentProps<"div"> & { value: string }) {
  const context = useTabsContext();

  if (context.value !== value) {
    return null;
  }

  return (
    <div role="tabpanel" className={cn("space-y-4", className)} {...props}>
      {children}
    </div>
  );
}

export { Tabs, TabsList, TabsTrigger, TabsContent };
