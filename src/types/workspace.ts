export interface AllowedFolder {
  id: number;
  path: string;
  writable: boolean;
  createdAt: string;
}

export interface FolderSuggestion {
  label: string;
  path: string;
}

export interface Project {
  id: number;
  name: string;
  path: string;
  repository: string;
  language: string;
  framework: string;
  description: string;
  notes: string;
  createdAt: string;
  updatedAt: string;
}

export type ProjectInput = Pick<Project, "name" | "path" | "repository" | "language" | "framework" | "description" | "notes">;

export interface DetectedProject {
  name: string;
  language: string;
  framework: string;
  repository: string;
  branch: string;
}

/** The backend rejects unknown fields, so send exactly the editable ones. */
export function toInput(p: Project): ProjectInput {
  const { name, path, repository, language, framework, description, notes } = p;
  return { name, path, repository, language, framework, description, notes };
}
