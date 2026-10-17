import { expect, finish, it } from "../../test/standaloneTest";
import { tabRowText } from "./tabRowModel";

it("selects title, optional description, and type label as row text", () => {
  expect(tabRowText({ title: "Build", description: "  Compile shaders  ", typeLabel: "  Worker " })).toEqual({
    title: "Build",
    description: "Compile shaders",
    typeLabel: "Worker",
  });
  expect(tabRowText({ title: "Shell", description: "  ", typeLabel: null })).toEqual({
    title: "Shell",
    description: null,
    typeLabel: null,
  });
});

finish();
