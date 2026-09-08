/** @module Interface orion:plugin/functions@1.0.0 **/
export function invoke(function_: string, input: string): string;
/**
 * # Variants
 * 
 * ## `"caller-input"`
 * 
 * ## `"internal"`
 */
export type ErrorClass = 'caller-input' | 'internal';
export interface PluginError {
  code: string,
  'class': ErrorClass,
  message: string,
}
