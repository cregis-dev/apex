"use client";

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { DashboardFilterOptions, DashboardRange } from "@/components/dashboard/types";

type DashboardFiltersProps = {
  range: DashboardRange;
  teamId: string;
  model: string;
  options: DashboardFilterOptions;
  onRangeChange: (value: DashboardRange) => void;
  onTeamChange: (value: string) => void;
  onModelChange: (value: string) => void;
};

const RANGE_OPTIONS: Array<{ value: DashboardRange; label: string }> = [
  { value: "1h", label: "Last 1 Hour" },
  { value: "24h", label: "Last 24 Hours" },
  { value: "7d", label: "Last 7 Days" },
  { value: "30d", label: "Last 30 Days" },
];

export function DashboardFilters({
  range,
  teamId,
  model,
  options,
  onRangeChange,
  onTeamChange,
  onModelChange,
}: DashboardFiltersProps) {
  const rangeLabel = RANGE_OPTIONS.find((option) => option.value === range)?.label ?? "Last 24 Hours";
  const teamLabel = teamId === "all" ? "All Teams" : teamId;
  const modelLabel = model === "all" ? "All Models" : model;

  return (
    <div className="flex flex-wrap items-center gap-3">
      <Select
        value={range}
        onValueChange={(value) => value && onRangeChange(value as DashboardRange)}
      >
        <SelectTrigger className="h-12 min-w-[220px] rounded-xl border-[#d5dde8] bg-white px-4 text-base font-medium text-[#111827] shadow-none">
          <SelectValue>{rangeLabel}</SelectValue>
        </SelectTrigger>
        <SelectContent className="rounded-2xl border border-[#ced6e1] bg-white p-2 text-[#111827] shadow-[0_10px_25px_rgba(15,23,42,0.12)]">
          {RANGE_OPTIONS.map((option) => (
            <SelectItem key={option.value} value={option.value} className="rounded-xl px-4 py-3 text-base font-medium">
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select value={teamId} onValueChange={(value) => onTeamChange(value ?? "all")}>
        <SelectTrigger className="h-12 min-w-[220px] rounded-xl border-[#d5dde8] bg-white px-4 text-base font-medium text-[#111827] shadow-none">
          <SelectValue>{teamLabel}</SelectValue>
        </SelectTrigger>
        <SelectContent className="rounded-2xl border border-[#ced6e1] bg-white p-2 text-[#111827] shadow-[0_10px_25px_rgba(15,23,42,0.12)]">
          <SelectItem value="all" className="rounded-xl px-4 py-3 text-base font-medium">All Teams</SelectItem>
          {options.teams.map((item) => (
            <SelectItem key={item} value={item} className="rounded-xl px-4 py-3 text-base font-medium">
              {item}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select value={model} onValueChange={(value) => onModelChange(value ?? "all")}>
        <SelectTrigger className="h-12 min-w-[220px] rounded-xl border-[#d5dde8] bg-white px-4 text-base font-medium text-[#111827] shadow-none">
          <SelectValue>{modelLabel}</SelectValue>
        </SelectTrigger>
        <SelectContent className="rounded-2xl border border-[#ced6e1] bg-white p-2 text-[#111827] shadow-[0_10px_25px_rgba(15,23,42,0.12)]">
          <SelectItem value="all" className="rounded-xl px-4 py-3 text-base font-medium">All Models</SelectItem>
          {options.models.map((item) => (
            <SelectItem key={item} value={item} className="rounded-xl px-4 py-3 text-base font-medium">
              {item}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
