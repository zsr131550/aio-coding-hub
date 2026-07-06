import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { RadioButtonGroup } from "../RadioButtonGroup";

describe("pages/providers/RadioButtonGroup", () => {
  it("uses theme-aware surfaces for inactive segmented buttons", async () => {
    const onChange = vi.fn();

    render(
      <RadioButtonGroup
        ariaLabel="Mode"
        items={[
          { value: "order", label: "按顺序" },
          { value: "ping", label: "按 Ping" },
        ]}
        value="order"
        onChange={onChange}
        fullWidth={false}
      />
    );

    expect(screen.getByRole("radiogroup", { name: "Mode" })).toHaveClass(
      "bg-surface-inset",
      "border-line-subtle"
    );
    expect(screen.getByRole("radio", { name: "按顺序" })).toHaveClass(
      "from-accent",
      "text-accent-foreground"
    );
    expect(screen.getByRole("radio", { name: "按 Ping" })).toHaveClass(
      "bg-transparent",
      "text-muted-foreground",
      "hover:bg-state-hover"
    );
    expect(screen.getByRole("radio", { name: "按 Ping" })).not.toHaveClass("bg-white");

    await userEvent.click(screen.getByRole("radio", { name: "按 Ping" }));
    expect(onChange).toHaveBeenCalledWith("ping");
  });
});
