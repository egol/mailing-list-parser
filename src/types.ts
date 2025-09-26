export interface Email {
  id: string;
  message_id: string;
  subject: string;
  from: string;
  to: string[];
  cc: string[];
  date: string;
  body: string;
  references: string[];
  in_reply_to?: string;
  patch_number?: number;
  patch_version?: number;
  is_patch: boolean;
  patch_filename?: string;
  commit_hash?: string;
}

export interface SearchResults {
  emails: Email[];
  total_count: number;
  has_more: boolean;
}

export interface SearchCriteria {
  query?: string;
  author?: string;
  subject_contains?: string;
  date_from?: string;
  date_to?: string;
  is_patch?: boolean;
  limit?: number;
  offset?: number;
}

export interface ThreadNode {
  email_id: string;
  parent_id?: string;
  children: string[];
  depth: number;
}

export interface Thread {
  id: string;
  root_email_id: string;
  subject: string;
  emails: ThreadNode[];
}
