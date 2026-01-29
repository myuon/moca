// Basic asm block with PushInt and Print
asm {
    __emit("PushInt", 42);
    __emit("Print");
};
