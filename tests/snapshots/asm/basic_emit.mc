// Basic asm block with PushInt and PrintInt
asm {
    __emit("PushInt", 42);
    __emit("PrintInt");
};
