// Basic asm block with PushInt and PrintDebug
asm {
    __emit("PushInt", 42);
    __emit("PrintDebug");
};
