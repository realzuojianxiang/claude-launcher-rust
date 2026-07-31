import { describe, expect, test } from "vitest";
import { render, screen } from "@testing-library/react";
import { FormField } from "./FormField";

describe("FormField", () => {
  test("form field connects labels, help and validation state to its control", () => {
    render(
      <FormField
        id="port"
        label="监听端口"
        hint="范围 1 到 65535"
        error="端口超出范围"
        status="error"
      >
        {(describedBy) => (
          <input id="port" aria-describedby={describedBy} aria-invalid="true" />
        )}
      </FormField>,
    );
    expect(screen.getByRole("textbox", { name: "监听端口" })).toHaveAttribute(
      "aria-describedby",
      "port-hint port-error",
    );
    expect(screen.getByRole("alert")).toHaveTextContent("端口超出范围");
  });

  test("success message uses status role", () => {
    render(
      <FormField id="name" label="名称" success="名称可用" status="success">
        {(describedBy) => <input id="name" aria-describedby={describedBy} />}
      </FormField>,
    );
    expect(screen.getByRole("status")).toHaveTextContent("名称可用");
    expect(screen.getByRole("textbox", { name: "名称" })).toHaveAttribute(
      "aria-describedby",
      "name-success",
    );
  });

  test("required label includes an indicator", () => {
    render(
      <FormField id="key" label="API Key" required>
        {(describedBy) => <input id="key" aria-describedby={describedBy} />}
      </FormField>,
    );
    expect(screen.getByText("*")).toBeInTheDocument();
  });
});
