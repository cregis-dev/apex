"use client";

import Image from "next/image";
import { KeyRound, Settings, Split, Users } from "lucide-react";

import apexLogo from "../../../apex.svg";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

const items = [
  { key: "dashboard", label: "Dashboard", icon: null, active: true },
  { key: "api-keys", label: "API Keys", icon: KeyRound, active: false },
  { key: "routing-rules", label: "Routing Rules", icon: Split, active: false },
  { key: "team-management", label: "Team Management", icon: Users, active: false },
  { key: "settings", label: "Settings", icon: Settings, active: false },
];

type DashboardSidebarProps = {
  statusLabel: string;
  statusMeta: string;
  onDisconnect: () => void;
};

export function DashboardSidebar({
  statusLabel,
  statusMeta,
  onDisconnect,
}: DashboardSidebarProps) {
  return (
    <aside className="flex w-full max-w-full flex-col rounded-[28px] border border-[#d9ccb7] bg-[#f6eedf] p-4 lg:min-h-[calc(100vh-3rem)] lg:w-72">
      <div className="mb-6 flex items-center gap-3 px-2">
        <div className="flex size-12 items-center justify-center overflow-hidden rounded-2xl border border-[#deceb1] bg-[#fff8eb]">
          <Image src={apexLogo} alt="Apex logo" className="size-9 object-contain" priority />
        </div>
        <div>
          <div className="text-sm font-medium uppercase tracking-[0.22em] text-[#6f624f]">
            Apex Gateway
          </div>
          <div className="text-xl font-semibold text-[#24140f]">Control Plane</div>
        </div>
      </div>

      <nav className="space-y-2">
        {items.map((item) => {
          const Icon = item.icon;
          return (
            <div
              key={item.key}
              className={cn(
                "flex items-center gap-3 rounded-2xl px-4 py-3 text-sm transition-colors",
                item.active
                  ? "bg-[#5b1f10] text-[#fff7eb]"
                  : "text-[#645646] hover:bg-[#efe3cf] hover:text-[#24140f]"
              )}
            >
              {Icon ? <Icon className="size-4" /> : <div className="size-4 rounded-full bg-[#c97b1e]" />}
              <span className="flex-1 font-medium">{item.label}</span>
              {!item.active ? (
                <span className="rounded-full bg-[#efe6d8] px-2 py-0.5 text-[11px] text-[#7b6a58]">
                  Soon
                </span>
              ) : null}
            </div>
          );
        })}
      </nav>

      <div className="mt-6 flex flex-1 items-end">
        <div className="w-full rounded-[24px] border border-[#dec6a6] bg-[#f3e4ca] p-4 text-sm text-[#3a2318]">
          <div className="text-[11px] font-medium uppercase tracking-[0.24em] text-[#8b6034]">
            Gateway session
          </div>
          <div className="mt-3 flex items-center justify-end gap-2">
            <span className="size-2.5 rounded-full bg-[#c97b1e]" />
            <span className="rounded-full bg-[#fff7eb] px-3 py-1 text-sm font-medium text-[#8d4d1a]">
              {statusLabel}
            </span>
          </div>
          <p className="mt-3 text-right text-xs leading-5 text-[#6c4b34]">{statusMeta}</p>
          <div className="mt-4 flex justify-end">
            <Button
              variant="outline"
              className="border-[#cda77b] bg-[#fff7eb] text-[#5b1f10] hover:bg-[#f7ebda]"
              onClick={onDisconnect}
            >
              Disconnect
            </Button>
          </div>
        </div>
      </div>
    </aside>
  );
}
