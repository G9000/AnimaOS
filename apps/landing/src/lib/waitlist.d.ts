export type AirtableConfig = {
    token: string;
    baseId: string;
    table: string;
};
export type Signup = {
    email: string;
    source: string;
};
export type ParseResult = {
    ok: true;
    signup: Signup;
} | {
    ok: false;
    status: number;
    error: string;
};
export declare function parseSignup(body: unknown): ParseResult;
export declare function saveToAirtable(config: AirtableConfig, signup: Signup, fetchImpl?: typeof fetch): Promise<void>;
//# sourceMappingURL=waitlist.d.ts.map