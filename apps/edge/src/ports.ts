// Which of the container's servers a path goes to: the worlds (vox-host) or the directory.
export const DIRECTORY_PORT = 7700;
export const HOST_PORT = 7790;
export const portFor = (path: string) => (path.startsWith("/w/") ? HOST_PORT : DIRECTORY_PORT);
